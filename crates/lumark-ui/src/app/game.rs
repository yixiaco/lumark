//! 游戏资产工作区的只读状态。
//!
//! 主窗口只在 Windows 暴露该功能；状态本身保持平台无关，便于后续扩展
//! 到其它能够加载 Oodle 的操作系统。

use std::path::PathBuf;

use gpui_kit::Entity;
use gpui_kit::component::input::InputState;
use lumark_browser::{BrowserModel, GameArchiveRow};
use lumark_settings::GameExportConfig;
use lumark_unreal::{DetectedGame, UnrealEngine};
use zeroize::Zeroizing;

/// 当前平台是否展示游戏资产功能。
pub(crate) const GAME_ASSETS_SUPPORTED: bool = cfg!(target_os = "windows");

/// 会话内 AES 密钥状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GameKeyState {
    /// 所有容器都未加密。
    NotNeeded,
    /// 密钥已解密并通过（或已保存）校验。
    Valid,
    /// 需要密钥但当前没有。
    Missing,
    /// 保存的密钥无效。
    Invalid,
}

/// 一个打开中的游戏。
pub(crate) struct GameSession {
    /// 设置中的游戏 ID。
    pub(crate) entry_id: String,
    /// 游戏库中保存的显示名称；扫描出的项目名可能只是子目录名（例如 "End"）。
    pub(crate) display_name: String,
    /// 扫描结果。
    pub(crate) game: DetectedGame,
    /// 仅内存中的 AES 密钥。
    pub(crate) key: Option<Zeroizing<String>>,
    /// 密钥状态。
    pub(crate) key_state: GameKeyState,
    /// 当前使用或尝试使用的 Oodle 动态库。
    pub(crate) oodle: Option<PathBuf>,
    /// 无版本属性所需的 `.usmap` 映射。
    pub(crate) mappings: Option<PathBuf>,
    /// 最近打开的容器。
    pub(crate) last_container: Option<PathBuf>,
}

/// 一个暂时切到后台的游戏会话及其独立浏览位置/历史。
pub(crate) struct InactiveGame {
    /// 游戏会话。
    pub(crate) session: GameSession,
    /// 切换前的浏览器快照（位置、条目、选择和历史栈）。
    pub(crate) browser: BrowserModel,
}

impl GameSession {
    /// 返回归档列表行。
    pub(crate) fn rows(&self) -> Vec<GameArchiveRow> {
        self.game
            .containers
            .iter()
            .map(|container| GameArchiveRow {
                name: container
                    .path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("container")
                    .to_string(),
                path: container.path.clone(),
                kind: container.kind.label().to_string(),
                size: container.size,
                encrypted: container.encrypted.unwrap_or(false),
            })
            .collect()
    }

    /// 当前容器是否属于本游戏。
    pub(crate) fn contains_container(&self, path: &std::path::Path) -> bool {
        self.game
            .containers
            .iter()
            .any(|container| container.path == path)
    }

    /// 返回引擎版本文本。
    pub(crate) fn engine_label(&self) -> &'static str {
        self.game.engine.label()
    }
}

/// 设置页的密钥状态。
#[derive(Debug, Clone)]
pub(crate) enum SetupKeyState {
    /// 尚未输入或提取。
    None,
    /// 正在提取。
    Extracting,
    /// 已找到并通过容器校验的密钥。
    Found(Zeroizing<String>),
    /// 提取失败。
    Error(String),
}

impl SetupKeyState {
    /// 返回可用密钥。
    pub(crate) fn key(&self) -> Option<&Zeroizing<String>> {
        match self {
            Self::Found(key) => Some(key),
            Self::None | Self::Extracting | Self::Error(_) => None,
        }
    }
}

/// 添加/编辑游戏的设置状态。
pub(crate) struct GameSetup {
    /// 编辑已有游戏时的 ID；`None` 表示新增。
    pub(crate) editing_id: Option<String>,
    /// 游戏目录输入。
    pub(crate) directory: Entity<InputState>,
    /// 游戏名输入。
    pub(crate) name: Entity<InputState>,
    /// AES 密钥输入（掩码）。
    pub(crate) key_input: Entity<InputState>,
    /// 目录扫描结果。
    pub(crate) scanned: Option<DetectedGame>,
    /// 目录扫描错误。
    pub(crate) scan_error: Option<String>,
    /// 密钥状态。
    pub(crate) key_state: SetupKeyState,
    /// 用户选择的 Oodle 库。
    pub(crate) oodle: Option<PathBuf>,
    /// 用户选择的 `.usmap` 映射。
    pub(crate) mappings: Option<PathBuf>,
    /// 每游戏导出配置。
    pub(crate) export: GameExportConfig,
    /// 导出过滤输入框。
    pub(crate) export_filter: Entity<InputState>,
    /// 是否有后台任务进行中。
    pub(crate) busy: bool,
    /// 行内提示。
    pub(crate) message: Option<String>,
    /// 手动指定的引擎版本；`None` 表示使用扫描结果。
    pub(crate) engine_override: Option<UnrealEngine>,
}

impl GameSetup {
    /// 当前展示的引擎版本。
    pub(crate) fn engine(&self) -> UnrealEngine {
        self.engine_override
            .or_else(|| self.scanned.as_ref().map(|game| game.engine))
            .unwrap_or_default()
    }

    /// 是否已扫描出可保存的游戏。
    pub(crate) fn ready(&self) -> bool {
        self.scanned.is_some() && !self.busy
    }
}
