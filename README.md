# OSKernel2026-Minux

## Description
**Team ID** : T2026105619910101

**Team Name**: Minux

**School** : South China University of Technology (华南理工大学) ， Future Tech （未来技术学院）

---

# Starry OS

*An experimental monolithic OS based on ArceOS*

[![GitHub Stars](https://img.shields.io/github/stars/Starry-OS/StarryOS?style=for-the-badge)](https://github.com/Starry-OS/StarryOS/stargazers)
[![GitHub Forks](https://img.shields.io/github/forks/Starry-OS/StarryOS?style=for-the-badge)](https://github.com/Starry-OS/StarryOS/network)
[![GitHub License](https://img.shields.io/github/license/Starry-OS/StarryOS?style=for-the-badge)](https://github.com/Starry-OS/StarryOS/blob/main/LICENSE)
[![Build status](https://img.shields.io/github/check-runs/Starry-OS/StarryOS/main?style=for-the-badge)](https://github.com/Starry-OS/StarryOS/actions)

## Supported Architectures

- [x] RISC-V 64
- [x] LoongArch64
- [x] AArch64
- [ ] x86_64 (work in progress)

## Features

TODO

## Quick Start

### 1. Clone repo

```bash
git clone --recursive https://github.com/Starry-OS/StarryOS.git
cd StarryOS
```

Or if you have already cloned it without `--recursive` option:

```bash
cd StarryOS
git submodule update --init --recursive
```

### 2. Install Prerequisites

#### A. Using Docker

We provide a prebuilt Docker image with all dependencies installed.

For users in mainland China, you can use the following image which includes optimizations like Debian packages mirrors and crates.io mirrors:

```bash
docker pull docker.cnb.cool/starry-os/arceos-build
docker run -it --rm -v $(pwd):/workspace -w /workspace docker.cnb.cool/starry-os/arceos-build
```

For other users, you can use the image hosted on GitHub Container Registry:

```bash
docker pull ghcr.io/arceos-org/arceos-build
docker run -it --rm -v $(pwd):/workspace -w /workspace ghcr.io/arceos-org/arceos-build
```

**Note:** The `--rm` flag will destroy the container instance upon exit. Any changes made inside the container (outside of the mounted `/workspace` volume) will be lost. Please refer to the [Docker documentation](https://docs.docker.com/) for more advanced usage.

#### B. Manual Setup

##### i. Install System Dependencies

This step may vary depending on your operating system. Here is an example based on Debian:

```bash
sudo apt update
sudo apt install -y build-essential cmake clang qemu-system
```

**Note:** Running on LoongArch64 requires QEMU 10. If the QEMU version in your Linux distribution is too old (e.g. Ubuntu), consider building QEMU from [source](https://www.qemu.org/download/).

##### ii. Install Musl Toolchain

1. Download files from [setup-musl releases](https://github.com/arceos-org/setup-musl/releases/tag/prebuilt)
2. Extract to some path, for example `/opt/riscv64-linux-musl-cross`
3. Add bin folder to `PATH`, for example:

   ```bash
   export PATH=/opt/riscv64-linux-musl-cross/bin:$PATH
   ```

##### iii. Setup Rust toolchain

```bash
# Install rustup from https://rustup.rs or using your system package manager

# Automatically download components via rustup
cd StarryOS
cargo -V
```

### 3. Prepare rootfs 准备磁盘镜像

#### 单盘模式

下载 Starry-OS 官方根文件系统作为启动盘：
```bash
# Default target: riscv64
make rootfs # 下载到 make/disk.img，启动后挂载为 /
# Explicit target
make ARCH=riscv64 rootfs
make ARCH=loongarch64 rootfs
```

This will download rootfs image from [Starry-OS/rootfs](https://github.com/Starry-OS/rootfs/releases) and set up the disk file for running on QEMU.

#### 双盘模式

启动后自动识别双盘：最后一块作为根 /，其余保留挂载到 /oscomp。

`COMPETITION=y` 模式下，系统默认执行 `src/init_competition.sh`，用于自动挂载评测盘、运行比赛入口并输出统一日志；如需快速定向回归，也可改用 `src/init_competition_focus.sh` 只跑指定测试集。

1. 评测盘
默认放置为 `make/test.img`（或任意路径，通过 `TEST_IMG` 指定）

- 下载比赛测评盘

   ```bash
   wget https://github.com/oscomp/testsuits-for-oskernel/releases/download/pre-20250615/sdcard-la.img.xz
   wget https://github.com/oscomp/testsuits-for-oskernel/releases/download/pre-20250615/sdcard-rv.img.xz

   ```

2. 从评测盘提取文件生成辅助根文件系统镜像：

   ```bash
   make aux            # 生成 make/disk.img（128MB）
   # 或指定评测盘路径
   make aux TEST_IMG=path/to/disk.img
   ```

   启动后 `make/disk.img` 挂载为 /，评测盘挂载到 /oscomp。

### 4. Build and run on QEMU 构建并运行

```bash
# Default target: riscv64
make build
# Explicit target（可选）
make ARCH=riscv64 build
make ARCH=loongarch64 build
```
#### 4.1 普通运行
```bash
make run LOG=info # QEMU 启动（查看日志）
# Run on QEMU (also rebuilds if necessary)
make ARCH=riscv64 run
make ARCH=loongarch64 run
```

#### 4.2 本地测试运行
```
# Competition mode
make COMPETITION=y run TEST_IMG=make/test.img
```

使用 `src/init_competition.sh` 作为默认入口脚本，启动后将辅助根文件系统挂载到 `/`，并将评测盘挂载到 `/oscomp`。

#### 4.3 官方评测机运行

##### 内核级改动后如何重新生成评测文件

官方评测使用顶层 kernel-rv / kernel-la 作为 ELF 提交产物。

本地默认运行链路与评测链路不完全相同；若修改了内核入口、链接脚本、平台地址布局或比赛模式相关配置，需要重新生成评测专用 ELF，避免继续使用仅适用于本地默认运行方式的旧产物。

常用命令：

```bash
# 重新生成评测机使用的 RISC-V 内核 ELF
make kernel-rv

# 重新生成评测机使用的 LoongArch 内核 ELF
make kernel-la

# 重新生成评测需要的全部提交文件
make all

# 重新准备辅助根文件系统镜像
make aux TEST_IMG=./sdcard-rv.img
```

##### 本地运行autotest评测机
参考 https://github.com/oscomp/autotest-for-oskernel

1. rv和la评测是分开的，可以基于当前要测试的架构准备 kernel-rv / kernel-la
2. disk.img、评测盘必须准备
3. 重新打包 autotest-for-oskernel/kernel.zip

Note:

1. Binary dependencies will be automatically built during `make build`.
2. You don't have to rerun `build` every time. `run` automatically rebuilds if necessary.
3. The disk file will **not** be reset between each run. As a result, if you want to switch to another architecture, you must run `make rootfs` with the new architecture before `make run`.

## TODO

- [ ] 本地缺少la架构官方评测构建链所需的完整的 LoongArch 工具链支持，无法稳定生成可用于官方评测的 kernel-la

## License

This project is now released under the Apache License 2.0. All modifications and new contributions in our project are distributed under the same license. See the [LICENSE](./LICENSE) and [NOTICE](./NOTICE) files for details.
