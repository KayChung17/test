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
