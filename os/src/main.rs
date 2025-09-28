//! The main module and entrypoint
//!
//! The operating system and app also starts in this module. Kernel code starts
//! executing from `entry.asm`, after which [`rust_main()`] is called to
//! initialize various pieces of functionality [`clear_bss()`]. (See its source code for
//! details.)
//!
//! We then call [`println!`] to display `Hello, world!`.

#![deny(missing_docs)]    // 强制要求所有公共项有文档注释，缺注释则编译报错
#![deny(warnings)]        // 把所有警告当作错误处理，严格编译
#![no_std]                // 禁用 Rust 标准库（关键！裸机环境没有标准库）
#![no_main]               // 禁用标准库的 `main` 入口（标准入口依赖操作系统）
#![feature(panic_info_message)]  // 启用 Rust 不稳定特性：获取 panic 时的详细信息

use core::arch::global_asm;  // 裸机编程的关键工具，用于在 Rust 代码中嵌入汇编代码（此处用于导入启动汇编 entry.asm）
use log::*;                  // 导入 log  crate 的所有宏（info!/warn!/error! 等日志）

#[macro_use]
// #[macro_use] 是一个模块级属性，用于将目标模块中定义的宏（macro_rules! 或声明宏）“提升” 到当前作用域，
// 让当前文件（或父模块）能直接使用这些宏，而无需通过 “模块名：：宏名” 的方式调用。
// 简单说：没有 #[macro_use]，模块里的宏是 “模块私有的”；加上它，宏就变成 “可跨模块直接使用的”。
mod console;        // 控制台模块：实现字符输出（往 QEMU 控制台打印内容）
mod lang_items;     // 语言项模块：补全 Rust 语言必需的“底层钩子”（如 panic 处理）
mod logging;        // 日志模块：初始化 log 库，对接 console 实现日志输出
mod sbi;            // SBI 模块：封装 RISC-V SBI 接口（与固件交互的底层函数）

#[path = "boards/qemu.rs"]
// #[path = "路径"] 是一个模块位置属性，用于手动指定 “模块对应的源代码文件路径”，打破 Rust 模块的 “默认文件查找规则”。
// Rust 模块的默认规则是：
// 若声明 mod xxx;，编译器会自动查找 xxx.rs（单个文件模块）或 xxx/mod.rs（目录模块）。
// 若文件不在默认位置，就需要用 #[path] 告诉编译器 “去哪里找这个模块的代码”。
mod board;          // 板级支持模块：适配 QEMU 模拟器的硬件配置（如退出接口）

global_asm!(include_str!("entry.asm"));
// 这是内核启动的 “第一站”，核心作用是：将 entry.asm 中的汇编代码嵌入到内核可执行文件中，作为内核的真正入口。
// 为什么需要汇编入口？
// Rust 函数无法直接作为裸机程序的入口，因为：
// 硬件复位后，CPU 处于 “原始状态”（寄存器未初始化、栈未建立），Rust 函数依赖栈来运行。
// 必须先完成一些 “前置工作”（如初始化栈、清零 BSS 段的准备），才能跳转到 Rust 代码。
// entry.asm 的核心工作（虽未贴代码，但可推断）：
// 初始化内核栈（设置栈指针寄存器 sp）；
// 调用 Rust 函数 rust_main()（本文件的核心入口）；
// 如果 rust_main() 意外返回（不应该发生），则让 CPU 停机（通过 SBI 接口）。

/// clear BSS segment
pub fn clear_bss() {
    extern "C" {
        fn sbss();
        fn ebss();
    }
    (sbss as usize..ebss as usize).for_each(|a| unsafe { (a as *mut u8).write_volatile(0) });
}
// extern "C" { ... }：
// 声明 “外部 C 链接的符号”，这些符号不是在 Rust 代码中定义的，而是由 “链接脚本（Linker Script）” 指定的内存地址。
// 链接脚本的作用：定义内核的内存布局（比如 .text 段放哪里、.bss 段从哪到哪），是裸机程序的 “内存地图”。
// 为什么要清零 BSS 段？
// ELF 标准规定：未初始化的全局 / 静态变量（如 static mut X: i32;）存放在 BSS 段，且执行前必须为 0。
// 如果不清零，这些变量会是内存中的随机垃圾值，导致程序行为不可预测。
// unsafe 块的必要性：
// 代码直接操作原始内存地址（a as *mut u8 是指向内存的原始指针），Rust 无法保证安全性（比如地址越界），所以必须用 unsafe 显式标记。
// write_volatile：确保编译器不优化这个写操作（避免因 “看似无用的写 0” 被编译器删掉）

/// the rust entry-point of os
#[no_mangle] // 禁止 Rust 编译器对函数名进行 “混淆（mangling）
pub fn rust_main() -> ! { //-> !：表示该函数 “永不返回”（发散函数）。内核入口函数一旦执行，理论上应该一直运行（管理硬件、调度任务），不会返回给调用者（汇编代码）
    // 1. 声明外部符号（由链接脚本定义的各段地址）
    extern "C" {
        fn stext(); // .text 段起始（代码段：存放内核指令）
        fn etext(); // .text 段结束
        fn srodata(); // .rodata 段起始（只读数据：如字符串常量）
        fn erodata(); // .rodata 段结束
        fn sdata(); // .data 段起始（已初始化数据：如 static X: i32 = 1;）
        fn edata(); // .data 段结束
        fn sbss(); // BSS 段起始（未初始化数据）
        fn ebss(); // BSS 段结束
        fn boot_stack_lower_bound(); // 启动栈的下界
        fn boot_stack_top(); // 启动栈的顶界（栈从高地址向低地址生长）
    }

    // 2. 初始化步骤
    clear_bss();          // 第一步：清零 BSS 段（必须先做，否则全局变量可能出错）
    logging::init();      // 第二步：初始化日志系统（后续才能用 info!/error!）

    // 3. 输出信息
    println!("[kernel] Hello, world!");  // 打印经典问候（用 console 模块的宏）
    
    // 打印各内存段的地址范围（调试用，确认内存布局正确）
    trace!(
        "[kernel] .text [{:#x}, {:#x})",  // {:#x}：以十六进制格式打印
        stext as usize,
        etext as usize
    );
    debug!(
        "[kernel] .rodata [{:#x}, {:#x})",
        srodata as usize, erodata as usize
    );
    info!(
        "[kernel] .data [{:#x}, {:#x})",
        sdata as usize, edata as usize
    );
    warn!(
        "[kernel] boot_stack top={:#x}, lower_bound={:#x}",
        boot_stack_top as usize, boot_stack_lower_bound as usize
    );
    error!(
        "[kernel] .bss [{:#x}, {:#x})",
        sbss as usize, ebss as usize
    );

    // 4. 退出 QEMU 模拟（CI 自动化测试用）
    use crate::board::QEMUExit;
    crate::board::QEMU_EXIT_HANDLE.exit_success(); // 通知 QEMU 成功退出
    // crate::board::QEMU_EXIT_HANDLE.exit_failure(); // 通知 QEMU 失败退出
}