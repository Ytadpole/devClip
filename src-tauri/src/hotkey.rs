//! 快捷键默认值与冲突预判 —— 阶段 5
//!
//! 解析交给 `tauri-plugin-global-shortcut`（它的 `Shortcut`
//! 实现了 `FromStr` 和 `Display`），本文件**不重复实现解析器**。
//! 只需要两样东西：
//!
//! - 每个平台的默认值（docs/05 给了分平台建议，用同一个会有问题）
//! - 已知被别的程序占用的组合，注册前先拦下

use tauri_plugin_global_shortcut::Shortcut;

/// 当前构建目标
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Windows,
    Linux,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Platform::MacOs
        } else if cfg!(target_os = "windows") {
            Platform::Windows
        } else {
            Platform::Linux
        }
    }
}

/// 每个平台的默认快捷键。
///
/// docs/05 的警告是 `⌘⇧V` 是个坏默认值（macOS 的
/// "Paste and Match Style"，Chrome / Slack / VSCode / JetBrains
/// 都占着），而分平台取值能避开各自主流应用的默认键
pub fn default_hotkey(p: Platform) -> &'static str {
    match p {
        Platform::MacOs => "Alt+Cmd+V",
        Platform::Windows => "Super+Shift+V",
        Platform::Linux => "Alt+Shift+V",
    }
}

/// 冲突规则。
///
/// `(平台, 组合, 原因)`。原因会直接显示给用户，所以要写清楚
/// **被谁占了**，而不只是「冲突了」
const CONFLICTS: &[(Platform, &str, &str)] = &[
    // docs/05 点名的坏默认值
    (
        Platform::MacOs,
        "Super+Shift+V",
        "macOS 的 ⌘⇧V 是「粘贴并匹配样式」，多数编辑器已占用",
    ),
    (
        Platform::MacOs,
        "Super+Q",
        "⌘Q 是「退出」，任何 Mac 应用都在用",
    ),
    (
        Platform::MacOs,
        "Super+W",
        "⌘W 是「关闭窗口」，几乎所有程序都占着",
    ),
    // 三个平台都被占：macOS ⌘W 关窗口，Windows/Linux Ctrl+W 关标签页
    (Platform::Windows, "Super+W", "Win+W 会关闭所有窗口"),
    (
        Platform::Windows,
        "Ctrl+W",
        "Ctrl+W 是「关闭标签页」，浏览器和编辑器都占着",
    ),
    (Platform::Linux, "Super+W", "Super+W 多数桌面环境用来关窗口"),
    (
        Platform::Linux,
        "Ctrl+W",
        "Ctrl+W 是「关闭标签页」，浏览器和编辑器都占着",
    ),
];

/// 这个组合在给定平台上是否已被别的程序占用
pub fn conflict(p: Platform, hk: &Shortcut) -> Option<&'static str> {
    CONFLICTS
        .iter()
        .find(|(cp, spec, _)| {
            if *cp != p {
                return false;
            }
            match spec.parse::<Shortcut>() {
                Ok(probe) => same(hk, &probe),
                // 表里写了个解析不了的组合。这是个 bug —— 表里每一项
                // 都有测试守着解析得通，所以这里等于「永远不匹配」
                // 而不是 panic。让测试去发现写错，别让运行时炸
                Err(_) => false,
            }
        })
        .map(|(_, _, why)| *why)
}

/// 两个组合是否等价。
///
/// 走 `id()` 而不是 `PartialEq`：`HotKey` 的 id 由
/// `(mods.bits() << 16) | key` 算出，修饰键**顺序无关**，
/// 所以 `Shift+Cmd+V` 和 `Cmd+Shift+V` 比相等 —— 正是我们要的
fn same(a: &Shortcut, b: &Shortcut) -> bool {
    a.id() == b.id()
}

/// 这个组合能不能注册。返回原因字符串表示不建议
pub fn check(p: Platform, hk: &Shortcut) -> Result<(), String> {
    // 没有修饰键的组合会吃掉所有软件的同名键。这是最糟的一类：
    // 注册「成功」了，但用户按 V 什么都不发生，比注册失败更难排查
    // 规范串是 `shift+alt+v` 这样用 `+` 分隔的。少于两段说明
    // 没有修饰键
    if hk.to_string().split('+').count() < 2 {
        return Err(format!("{hk} 没有修饰键，会抢走所有软件的这个键"));
    }
    match conflict(p, hk) {
        Some(why) => Err(format!("{why}。换一个组合试试")),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hk(s: &str) -> Shortcut {
        s.parse().unwrap_or_else(|e| panic!("{s:?} 解析失败: {e}"))
    }

    /// 表里每一项都必须解析得通。写错格式的组合会静默变成一条
    /// 永不触发的规则 —— 这是最难发现的一类错
    #[test]
    fn conflict_table_is_parseable() {
        for (p, spec, why) in CONFLICTS {
            hk(spec);
            assert!(!why.is_empty(), "{p:?} 的 {spec} 缺原因");
        }
    }

    /// 表里每一项都必须真的能命中自己
    #[test]
    fn conflict_table_matches_itself() {
        for (p, spec, why) in CONFLICTS {
            assert_eq!(conflict(*p, &hk(spec)), Some(*why), "{spec} 匹配不上自己");
        }
    }

    /// docs/05 点名的坏默认值必须被认出来
    #[test]
    fn detects_documented_macos_landmine() {
        assert!(conflict(Platform::MacOs, &hk("Super+Shift+V")).is_some());
        // 但它是 Linux 的推荐默认值，不能在 Linux 上报错
        assert!(conflict(Platform::Linux, &hk("Super+Shift+V")).is_none());
    }

    /// 三个平台的默认值都不能撞上自己的冲突表 ——
    /// 否则用户第一次启动就看到一个「你选的键不能用」的对话框
    #[test]
    fn defaults_have_no_conflict() {
        for p in [Platform::MacOs, Platform::Windows, Platform::Linux] {
            let d = hk(default_hotkey(p));
            assert_eq!(check(p, &d), Ok(()), "{p:?} 的默认值不该冲突");
        }
    }

    /// 修饰键顺序不该影响判定
    #[test]
    fn modifier_order_is_irrelevant() {
        for s in ["Super+Shift+V", "Shift+Super+V", "shift+super+v"] {
            assert!(
                conflict(Platform::MacOs, &hk(s)).is_some(),
                "{s:?} 应该被认成 ⌘⇧V"
            );
        }
    }

    /// 裸键是最糟的一类：注册会「成功」，但按 V 什么都不发生
    #[test]
    fn rejects_bare_key() {
        let r = check(Platform::Linux, &hk("V"));
        assert!(r.is_err(), "裸键不该放行");
        assert!(r.unwrap_err().contains("修饰键"), "要说清为什么");
    }

    #[test]
    fn accepts_ordinary_combos() {
        for s in ["Alt+Shift+V", "Ctrl+Shift+Space", "Alt+F4"] {
            let h = hk(s);
            if let Err(e) = check(Platform::Linux, &h) {
                // 只在确实撞表时才该失败
                assert!(
                    conflict(Platform::Linux, &h).is_some(),
                    "{s:?} 意外被拒: {e}"
                );
            }
        }
    }

    /// Ctrl+W 在 Windows/Linux 冲突，在 macOS 不冲突
    /// （macOS 上那是 Ctrl+W，不是关闭标签页）
    #[test]
    fn ctrl_w_conflicts_per_platform() {
        let w = hk("Ctrl+W");
        assert!(conflict(Platform::Windows, &w).is_some());
        assert!(conflict(Platform::Linux, &w).is_some());
        assert!(conflict(Platform::MacOs, &w).is_none());
    }

    /// 分平台默认值互不相同 —— 同一个值在三台机器上行为不一样是 bug
    #[test]
    fn defaults_differ_per_platform() {
        let mac = hk(default_hotkey(Platform::MacOs));
        let win = hk(default_hotkey(Platform::Windows));
        let lin = hk(default_hotkey(Platform::Linux));
        assert!(!same(&mac, &lin), "macOS 与 Linux 默认值不该相同");
        assert!(!same(&win, &lin), "Windows 与 Linux 默认值不该相同");
    }
}
