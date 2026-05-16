# StarryOS 高并发与网络优化修改报告 (Round 1)

## 概述

本报告记录了针对 oscomp 测试套件对 StarryOS (oskernel2026-minux) 进行的第一轮系统性优化。修改覆盖调度器、SMP 多核、网络协议栈、系统调用路径、IPC 消息队列、IPv6 支持及 timerfd 实现。

---

## Phase 0: 基础设施 — Vendor 依赖库

将 `axsched`、`axtask` 和 `axnet-ng` 三个核心 crate 从 crates.io 拷贝到 `third_party/`，通过 `[patch.crates-io]` 覆盖，实现对调度器和网络协议栈的深度定制。

**涉及文件:**
- `third_party/axsched/` (新增) — 调度器框架，新增 `rt.rs` 实现 RtScheduler
- `third_party/axtask/` (新增) — 任务管理，新增 `sched-rt` feature
- `third_party/axnet-ng/` (新增) — 网络协议栈，修改 ListenTable 和 TcpSocket
- `Cargo.toml` — 添加 `[patch.crates-io]` 覆盖条目
- `kernel/Cargo.toml` — 启用 `smp`/`ipi` feature，切换 `sched-rr` → `sched-rt`

---

## Phase 1: P0 — 调度器优先级 + SMP (决定分数段位)

### P0.1: SCHED_FIFO + SCHED_RR 优先级调度

**灵感来源:** Demikernel (SOSP 2021) 的三级优先级调度 + Unikraft (EuroSys 2021) 的可插拔调度器 API

**实现方案:**
1. 在 `third_party/axsched/src/rt.rs` 新增 `RtScheduler<T, N_PRIO>` — 100 级优先级 O(1) bitmap 就绪队列
2. SCHED_FIFO: 无时间片抢占，同优先级 FCFS
3. SCHED_RR: 有时间片轮转，同优先级 RR
4. SCHED_OTHER: 优先级 0，对应 nice 值 (-20..19)
5. `RtTask` 新增 `priority`、`policy`、`time_slice` 原子字段

**涉及文件:** `third_party/axsched/src/rt.rs` (255行新增)、`third_party/axtask/src/lib.rs` (60行修改)

**影响测试:** cyclictest (全部4项)、LTP sched (全部)

### P0.2: SMP 多核支持与跨核信号 IPI

**灵感来源:** EbbRT (OSDI 2016) per-core 数据结构 + ZygOS (SOSP 2017) work-stealing

**实现方案:**
1. 启用 `smp` 和 `ipi` feature (kernel/Cargo.toml)
2. 跨核信号传递 (`kernel/src/task/signal.rs`): 当目标线程运行在其他 CPU 时，通过 `axhal::irq::send_ipi` 发送 IPI 中断目标 CPU，让其检查并处理信号
3. 单核时 (`MAX_CPU_NUM == 1`) 无 IPI 开销

**涉及文件:** `kernel/src/task/signal.rs` (+13行)、`kernel/Cargo.toml`

**影响测试:** hackbench、lat_ctx 多进程、iperf3 -P5、cyclictest -t8

---

## Phase 2: P1 — 显著提升并发分数

### P1.3: SO_REUSEADDR 支持

**灵感来源:** MegaPipe (NSDI 2013) 分区监听 socket

**实现方案:**
1. `third_party/axnet-ng/src/listen_table.rs`: `can_listen()` 和 `listen()` 增加 `reuse_addr` 参数
   - `can_listen(port, reuse_addr)`: `reuse_addr || port_is_free`
   - `listen(endpoint, reuse_addr)`: 若端口被占用且 `reuse_addr=true`，覆盖已有 listener
2. `third_party/axnet-ng/src/tcp.rs`: bind 时传递 `reuse_address()` 标志到 ListenTable
3. `third_party/axnet-ng/src/general.rs`: 新增 `ReuseAddress` 原子标志存储

**涉及文件:**
- `third_party/axnet-ng/src/listen_table.rs` — listen()/can_listen() 逻辑修改
- `third_party/axnet-ng/src/tcp.rs` — 传递 reuse_addr 标志
- `third_party/axnet-ng/src/general.rs` — ReuseAddress 原子存储
- `third_party/axnet-ng/src/options.rs` — ReuseAddress 选项枚举

**影响测试:** iperf3 PARALLEL、netperf TCP_CRR、多轮并发测试

### P1.4: 系统调用路径优化

**灵感来源:** FlexSC (OSDI 2010) 无异常 syscall + UKL 直接函数调用

**实现方案:**
在 `handle_syscall()` 入口处预提取全部 6 个参数 (`a0..a5`)，避免每个 handler 分支重复调用 `uctx.argN()`。所有 syscall handler 从 `uctx.argN()` 改为直接使用预提取的 `a0..a5`。

**涉及文件:** `kernel/src/syscall/mod.rs` (~600行变更，1012行 diff)

**影响测试:** UnixBench syscall (基线 15000 lps)、lmbench lat_syscall

### P1.5: 消息队列阻塞模式

**灵感来源:** 复用内核已有的 WaitQueue 机制 (epoll/futex/sleep 均使用 `block_on` + `WaitQueue`)

**实现方案:**
1. `MessageQueue` 新增 `send_wait_queue` 和 `recv_wait_queue` (Arc<WaitQueue>)
2. `msgsnd` 队列满且无 `IPC_NOWAIT`: `recv_wq.wait_until(...)` 阻塞等待空间
3. `msgrcv` 队列空且无 `IPC_NOWAIT`: `recv_wq.wait_until(...)` 阻塞等待消息
4. 操作成功后唤醒对方 WaitQueue (`notify_all`)
5. `IPC_RMID` 删除队列时清空所有等待者，返回 `EIDRM`

**涉及文件:** `kernel/src/syscall/ipc/msg.rs` (+~150行阻塞逻辑，删除旧的 TODO/warn! 占位)

**影响测试:** LTP msg* 全部 (阻塞 msgrcv/msgsnd 测试)

---

## Phase 3: P2 — 完善功能覆盖

### P2.7: IPv6 基础支持

**实现方案:**
`AF_INET6` socket/bind/connect 基础支持，通过 match arm 扩展实现:
- `sys_socket`: `AF_INET6` 与 `AF_INET` 走相同路径创建 TcpSocket/UdpSocket
- `axnet-ng/src/lib.rs`: 添加 IPv6 ethernet 帧处理
- 注意: 移除了不必要的 IPv6 loopback 地址以规避 `IFACE_MAX_ADDR_COUNT=2` 限制

**涉及文件:** `kernel/src/syscall/net/socket.rs`、`third_party/axnet-ng/src/lib.rs`

### P2.8: timerfd 实现

**实现方案:**
参照 EventFd/SignalFd 模式创建 TimerFd 文件类型:
- `kernel/src/file/timerfd.rs` (241行新增): TimerFd struct + FileLike + Pollable trait 实现
- `kernel/src/syscall/fs/timerfd.rs` (78行新增): `sys_timerfd_create`/`sys_timerfd_settime`/`sys_timerfd_gettime`
- 支持: CLOCK_MONOTONIC/CLOCK_REALTIME/CLOCK_BOOTTIME、TFD_TIMER_ABSTIME、阻塞 read、one-shot/recurring 定时器
- `kernel/src/syscall/mod.rs`: 将 timerfd 从 dummy dispatch 改为真实 handler

**涉及文件:** `kernel/src/file/timerfd.rs`、`kernel/src/syscall/fs/timerfd.rs`、`kernel/src/file/mod.rs`、`kernel/src/syscall/fs/mod.rs`

### 附加: setpriority/getpriority 实现

在 `ProcessData` 中新增 `nice: AtomicI32` 字段，实现完整的 `sys_setpriority`，支持 PRIO_PROCESS/PRIO_PGRP/PRIO_USER。

**涉及文件:** `kernel/src/task/mod.rs` (+11行)、`kernel/src/syscall/task/schedule.rs` (+97行)

---

## 测试套件

为每个功能创建了独立的 RISC-V musl 静态链接测试程序:

| 测试文件 | 测试内容 | 行数 |
|---------|---------|------|
| `benchmarks/test_sched.c` | SCHED_FIFO/RR 策略设置与验证 | 79 |
| `benchmarks/test_reuseaddr.c` | SO_REUSEADDR listen() 碰撞检测 | 76 |
| `benchmarks/test_syscall.c` | 100K 迭代 syscall 吞吐量 | 77 |
| `benchmarks/test_msg.c` | 消息队列阻塞 fork/send/recv | 83 |
| `benchmarks/test_timerfd.c` | timerfd 创建/设置/读取 | 111 |
| `benchmarks/test_ipv6.c` | AF_INET6 socket 创建与绑定 | 52 |
| `benchmarks/run_tests.py` | pexpect QEMU 自动化测试框架 | 102 |

---

## oscomp 基线数据分析

详细分析了 sdcard-rv.img (4.3GB) 中的测试结构:

- **UnixBench**: 26+ 子测试，有明确的 `index.base` 基线 (SPARCstation 20-61)，公式 `index = (实测/基线)*10`
- **LTP**: 2842 个测试用例，pass/fail (退出码)
- **busybox**: ~55 条命令，pass/fail
- **libctest**: ~107 个 musl libc 测试
- **cyclictest**: 4 场景，无 index.base 基线但有延迟阈值要求
- **iperf3**: 6 场景，2 秒跑
- **netperf**: 5 场景，1 秒跑
- **lmbench**: ~20 项延迟/带宽测量
- **iozone**: 7 组文件 IO 测试

---

## 论文参考汇总

| 方向 | 论文 | 会议 | 核心技术 |
|------|------|------|----------|
| 调度器 | Demikernel | SOSP 2021 | Rust coroutine 调度，12周期开销，三级优先级 |
| 调度器 | Unikraft | EuroSys 2021 (Best Paper) | 可插拔调度器微库，Kconfig 策略选择 |
| 多核 | EbbRT | OSDI 2016 | Per-core event loop, lock-free 数据结构 |
| 多核 | ZygOS | SOSP 2017 | Work-stealing + shared-memory + IPI |
| 网络 | MegaPipe | NSDI 2013 | 分区监听 socket，消除 accept 队列竞争 |
| 网络 | mTCP | NSDI 2014 | Lock-free per-core TCP，流亲和性，批处理 |
| Syscall | FlexSC | OSDI 2010 | 无异常 syscall，shared-memory syscall page |

---

## 代码统计

| 类别 | 行数 |
|------|------|
| 新增文件（third_party + kernel + benchmarks） | ~2640 行 |
| 修改文件 | 12 个文件，+626/-464 行 |
| 删除 TODO/warn! 占位 | ~15 处 |

---

## 下一步

1. 在 QEMU 中挂载 sdcard-rv.img 作为第二磁盘
2. 运行 oscomp 测试套件（UnixBench、cyclictest、iperf3、netperf、LTP 等）
3. 收集 UnixBench index 分数，对比优化前后
4. 根据测试结果进行针对性修复和调优
