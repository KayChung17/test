# 文件系统

## 修改2
本轮主要完成了两件事：先定位旧静默点的根因，再把对应修复收尾清理干净。

### 旧静默点定位

之前长时间静默的根因并不是 `waitpid` 卡死，而是 `lmbench_all` 在 `/var/tmp/XXX` 这条 file-backed mmap 路径上持续触发 page fault。

相关热路径主要在：

- `kernel/src/syscall/mm/mmap.rs`
- `kernel/src/task/user.rs`
- `kernel/src/mm/aspace/backend/file.rs`
- `third_party/arceos/axfs/src/highlevel/file.rs`
- `third_party/arceos/axfs/src/fs/ext4/fs.rs`

问题的本质是：

- 页缓存淘汰时会同步写回 ext4；
- ext4 整体又被全局 mutex 串行化；
- 因此 page fault 热路径里会反复出现  
  “淘汰脏页 -> 同步 ext4 写回 -> 再读入新页”  
  这样的超慢循环。

所以旧静默点本质上是 file-backed mmap 与缓存淘汰/写回路径叠加后的性能问题，而不是进程等待逻辑本身异常。

### 实质修复

本轮核心修复主要落在：

- `third_party/arceos/axfs/src/highlevel/file.rs`

主要做了两类修改。

#### 1. 收紧 CachedFile 共享状态复用

对于非 tmpfs 文件，改为按照 `(device, inode)` 统一复用共享缓存状态，避免同一文件在 reopen 后落到彼此不一致的 cache state。

#### 2. 将脏页淘汰改为延迟写回

在 `CachedFileShared` 中增加 `dirty_evicted`，并调整写回策略：

- `evict_cache()` 在淘汰脏页时不再直接执行 `file.write_at()`；
- 改为先保存脏页快照；
- `page_or_insert()` 在缺页回填时优先从快照恢复；
- `sync()` 和最终 `drop` 时，再统一把 LRU 中脏页与 `dirty_evicted` 一并落盘。

这样就把最慢的同步 ext4 写回，从 page-fault 热路径中移走了。

### 验证结果

修复后，测试已经越过原先的 `file system latency` / `/var/tmp/XXX` 静默点。

这说明本轮针对旧卡点的修复是有效的，原问题已经被实质解决。

### 收尾清理

本轮还清理了为定位旧问题临时加入的诊断日志，只保留真正有效的修复。

清理涉及的文件包括：

- `kernel/src/syscall/fs/io.rs`
- `kernel/src/syscall/mm/mmap.rs`
- `kernel/src/task/user.rs`
- `kernel/src/syscall/task/clone.rs`
- `kernel/src/syscall/task/wait.rs`
- `kernel/src/task/ops.rs`

### 当前状态

当前保留下来的有效修复集中在：

- `third_party/arceos/axfs/src/highlevel/file.rs`

也就是说，这一轮已经完成了对旧静默点的定位、修复和清理；后续如果还有新的卡点，需要作为新的问题单独继续分析。

## 修改3

本轮主要处理的是越过旧文件系统静默点之后，新暴露出来的 LTP cgroup 测试卡点，并顺手把本轮新增的诊断输出再次收尾清理。

### 新卡点定位

测试越过原先的 `file system latency` 卡点后，日志最终停在：

- `RUN LTP CASE cgroup_fj_proc`

但继续回看前序输出可以确认，真正先暴露的问题并不是 `cgroup_fj_proc` 本身，而是两类基础问题：

#### 1. cgroup / cgroup2 挂载入口直接失败

LTP 在进入 cgroup 相关用例前，挂载 V1/V2 cgroup 时直接返回 `ENODEV`。

根因在：

- `kernel/src/syscall/fs/mount.rs`

原先这里只接受：

- `tmpfs`
- `/dev/...` 对应的额外挂载

因此 `fs_type == "cgroup"` 或 `"cgroup2"` 时会直接落到 `NoSuchDevice`，导致用户态看到 `ENODEV`。

#### 2. LTP shell 用例启动上下文不对

同时，cgroup 相关 shell case 还出现了：

- `can't open 'cgroup_lib.sh'`
- `can't open 'cgroup_fj_common.sh'`
- `tst_brk: not found`

这说明问题不只是内核侧缺少 cgroup mount 支持，LTP 本身的启动方式也不对。

根因在：

- `src/init_competition.sh`

原先总入口是在 `/oscomp/glibc` 下直接执行 `*_testcode.sh`，而 LTP 的 shell 用例实际上依赖：

- 正确的 `LTPROOT`
- 正确的 `PATH`
- 从 LTP 根目录启动 `runltp`

否则脚本内通过相对路径 `source` 的辅助文件和辅助命令都无法正常解析。

### 实质修复

本轮修改主要落在：

- `src/init_competition.sh`
- `kernel/src/syscall/fs/mount.rs`
- `kernel/src/pseudofs/proc.rs`

#### 1. 修正 LTP 启动方式

在 `src/init_competition.sh` 中，把 LTP 改成专门分支处理：

- 设置 `LTPROOT=/oscomp/glibc/ltp`
- 把 `ltp/testcases/bin` 加入 `PATH`
- 切到 `LTPROOT`
- 直接执行 `./runltp`

这样做之后，LTP 的 shell 用例终于能在它自己预期的运行环境里启动，不再因为辅助脚本和辅助命令解析失败而提前假死或假失败。

#### 2. 给 cgroup / cgroup2 增加最小 mount 支持

在 `kernel/src/syscall/fs/mount.rs` 中，把：

- `cgroup`
- `cgroup2`

接入现有 pseudo-fs 路径，使其不再在 mount 入口直接返回 `ENODEV`。

这一轮只做最小打通，目标是先让测试真正进入后续功能路径，而不是在 mount 入口就被拒绝。

#### 3. 补齐 `/proc/filesystems` 声明

在 `kernel/src/pseudofs/proc.rs` 中补上：

- `nodev cgroup`
- `nodev cgroup2`

让用户态看到的文件系统声明与当前实际支持保持一致。

### 收尾清理

这一轮在验证完成后，又把为了继续定位问题临时加上的诊断输出清理掉了，只保留真正有效的行为修改。

主要清理了：

- `third_party/arceos/axfs/src/highlevel/file.rs` 中缓存淘汰/回填/同步相关日志
- `kernel/src/file/fs.rs` 中 nonblocking read/write poll 日志
- `kernel/src/syscall/fs/io.rs` 中 `fdatasync` 进出日志
- `kernel/src/syscall/task/ctl.rs` 中 unsupported `prctl` 告警

### 当前状态

这一轮的结果是：

- LTP 不再以错误的启动上下文运行；
- `cgroup` / `cgroup2` 不再在 mount 入口直接返回 `ENODEV`；
- 输出已经再次清理干净，没有保留本轮临时诊断日志。

也就是说，这一轮完成的是“新卡点的基础打通 + 调试输出收尾”；如果后续 cgroup 相关测试还存在失败，接下来面对的就会是更真实的功能缺口，而不再是环境或入口问题。
