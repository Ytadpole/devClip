//! 设置持久化 —— 阶段 5（快捷键）→ 阶段 7（全部设置项）
//!
//! 落盘的是 `Stored`：每个字段都是 Option，读出来的缺字段用默认值补 ——
//! 以后加设置项时，老配置文件缺字段是常态，不能因此读不出来。

use serde::{Deserialize, Serialize};
use std::path::Path;

/// 落盘的内容。
///
/// `Option<String>` 而不是「空串表示没设过」：空串不是合法快捷键，
/// 用户手改配置文件时写空串进来，语义应该是「没设」，
/// 而不是「有个我读不懂的快捷键」。
/// 数值/布尔项同理 —— `None` = 用默认值，`0` 是合法值
/// （比如 retention_days = 0 表示「立即过期」），不能用 0 当「没设」
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Stored {
    pub hotkey: Option<String>,
    /// 历史最多保留多少条
    pub max_items: Option<i64>,
    /// 历史保留天数，0 = 立即过期（收藏项除外）
    pub retention_days: Option<i64>,
    /// 单张图片入库的字节上限
    pub max_image_bytes: Option<i64>,
    /// dark / light / system。UI 尚未消费，先存着
    pub theme: Option<String>,
    /// 敏感内容是否到期自动删除（docs/03 的 sensitive.auto_expire）
    pub sensitive_auto_expire: Option<bool>,
}

/// 读设置。文件不存在或读不懂都返回默认值
///
/// **读不懂不等于崩**。设置文件坏了就当没设过、让用户重新确认；
/// 因为一个逗号写错就让整个应用起不来明显更糟
pub fn load(path: &Path) -> Stored {
    match std::fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|e| {
            eprintln!("设置文件读不出来，按默认值处理 {}: {e}", path.display());
            Stored::default()
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Stored::default(),
        Err(e) => {
            eprintln!("设置文件打不开 {}: {e}", path.display());
            Stored::default()
        }
    }
}

/// 写设置。**原子写**：先写临时文件再改名。
///
/// 直接覆盖的话，程序在写到一半被杀（磁盘满、用户强制退出）
/// 会留下半个 JSON，下次启动读不出来 —— 而那正是最需要读出来的
/// 时候。临时名带进程号，避免两个实例互相覆盖
pub fn save(path: &Path, s: &Stored) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let json = serde_json::to_string_pretty(s).map_err(|e| format!("序列化失败: {e}"))?;

    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, json).map_err(|e| format!("写入失败: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        // 改名失败就把临时文件清掉，别留下垃圾
        let _ = std::fs::remove_file(&tmp);
        format!("保存失败: {e}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("devclip-settings-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn missing_file_gives_defaults() {
        let p = dir("missing").join("settings.json");
        assert_eq!(load(&p), Stored::default());
    }

    /// 设置文件坏了要让用户重新确认，而不是整个应用起不来
    #[test]
    fn corrupt_file_falls_back_to_defaults() {
        let p = dir("corrupt").join("settings.json");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{ this is not json ").unwrap();
        assert_eq!(load(&p), Stored::default());
    }

    /// 字段缺失也要能用 —— 以后加设置项时老配置文件缺字段是常态
    #[test]
    fn partial_file_is_accepted() {
        let p = dir("partial").join("settings.json");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{}").unwrap();
        assert_eq!(load(&p), Stored::default());
    }

    #[test]
    fn roundtrip() {
        let p = dir("roundtrip").join("settings.json");
        let want = Stored {
            hotkey: Some("Ctrl+Shift+Space".into()),
            ..Default::default()
        };
        save(&p, &want).unwrap();
        assert_eq!(load(&p), want);
    }

    /// 全字段落盘再读回来，一个都不能丢。
    /// settings_to_stored 建立在「Stored 能完整表达 Settings」上，
    /// 少一个字段就是静默重置
    #[test]
    fn roundtrip_all_fields() {
        let p = dir("roundtrip-all").join("settings.json");
        let want = Stored {
            hotkey: Some("Ctrl+Shift+Space".into()),
            max_items: Some(500),
            retention_days: Some(7),
            max_image_bytes: Some(1024),
            theme: Some("light".into()),
            sensitive_auto_expire: Some(false),
        };
        save(&p, &want).unwrap();
        assert_eq!(load(&p), want);
    }

    /// 首次运行时目录还不存在
    #[test]
    fn save_creates_parent_directory() {
        let p = dir("mkdir").join("deep").join("settings.json");
        assert!(!p.parent().unwrap().exists());
        save(&p, &Stored::default()).unwrap();
        assert!(p.exists());
    }

    /// 原子写不该留下临时文件
    #[test]
    fn save_leaves_no_temp_file() {
        let d = dir("notemp");
        let p = d.join("settings.json");
        save(
            &p,
            &Stored {
                hotkey: Some("Alt+V".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n != "settings.json")
            .collect();
        assert!(leftovers.is_empty(), "留下垃圾文件：{leftovers:?}");
    }

    /// 覆盖而不是追加
    #[test]
    fn overwrites_existing_file() {
        let p = dir("overwrite").join("settings.json");
        save(
            &p,
            &Stored {
                hotkey: Some("Alt+V".into()),
                ..Default::default()
            },
        )
        .unwrap();
        save(
            &p,
            &Stored {
                hotkey: Some("Ctrl+Shift+Space".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(load(&p).hotkey.as_deref(), Some("Ctrl+Shift+Space"));
        let raw = std::fs::read_to_string(&p).unwrap();
        assert_eq!(raw.matches("hotkey").count(), 1, "内容：{raw}");
    }
}
