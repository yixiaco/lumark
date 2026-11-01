//! 游戏资产动作：扫描目录、保存游戏、提取密钥和打开容器。

use super::super::*;
use crate::i18n::tr;
use lumark_settings::{AesKeySource, GameLibraryEntry};
use lumark_unreal::{
    KeyStage, Mappings, UnrealEngine, extract_key_for_game, scan_game, verify_container_key,
};
use zeroize::Zeroizing;

mod editor;

impl LumarkApp {
    /// 进入游戏资产工作区：主窗口显示游戏库列表。
    ///
    /// 入口不再自动打开上次游戏；列表页保留已打开的游戏会话，用户点“打开”
    /// 时才进入容器浏览，返回列表也不会丢失浏览位置。
    pub(crate) fn open_game_assets(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !GAME_ASSETS_SUPPORTED {
            self.status = tr(
                "当前平台暂不支持游戏资产功能",
                "Game assets are unavailable on this platform",
            )
            .to_string();
            cx.notify();
            return;
        }
        if self.game_session.is_none() && !self.game_library {
            self.remember_previous_directory();
        }
        self.game_library = true;
        self.screen = Screen::Browser;
        cx.notify();
    }

    /// 返回游戏库列表页；保留当前游戏会话和浏览位置。
    pub(crate) fn back_to_game_library(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.persist_open_games();
        self.game_library = true;
        self.screen = Screen::Browser;
        cx.notify();
    }

    /// 打开“添加游戏”编辑窗口。
    pub(crate) fn open_game_setup_new(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !GAME_ASSETS_SUPPORTED {
            self.status = tr(
                "当前平台暂不支持游戏资产功能",
                "Game assets are unavailable on this platform",
            )
            .to_string();
            cx.notify();
            return;
        }
        self.spawn_game_editor(None, window, cx);
    }

    /// 记住进入游戏资产前的文件系统位置。
    fn remember_previous_directory(&mut self) {
        if let BrowserLocation::FileSystem(path) = &self.browser.location {
            self.game_previous_dir = Some(path.clone());
        }
    }

    /// 打开新增游戏设置页。
    pub(crate) fn start_game_setup(
        &mut self,
        entry: Option<&GameLibraryEntry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let directory = new_input(window, cx, tr("游戏安装目录", "Game install folder"), false);
        let name = new_input(window, cx, tr("游戏名称", "Game name"), false);
        let key_input = new_input(
            window,
            cx,
            tr("AES 密钥（十六进制）", "AES key (hex)"),
            true,
        );
        let mut setup = GameSetup {
            editing_id: entry.map(|entry| entry.id.clone()),
            directory,
            name,
            key_input,
            scanned: None,
            scan_error: None,
            key_state: SetupKeyState::None,
            oodle: entry.and_then(|entry| entry.oodle_dll.clone()),
            mappings: entry.and_then(|entry| entry.mappings.clone()),
            busy: false,
            message: None,
            engine_override: entry.and_then(|entry| UnrealEngine::from_setting(&entry.engine)),
        };
        if let Some(entry) = entry {
            set_input(&setup.directory, &entry.root.to_string_lossy(), window, cx);
            set_input(&setup.name, &entry.name, window, cx);
            if let Some(blob) = &entry.aes_key
                && let Ok(key) = platform::secret::unprotect_text(blob)
            {
                setup.key_state = SetupKeyState::Found(Zeroizing::new(key));
            }
        }
        self.game_setup = Some(setup);
        if let Some(entry) = entry {
            let root = entry.root.clone();
            self.scan_game_directory(root, window, cx);
        }
        cx.notify();
    }

    /// 编辑当前会话对应的游戏：在独立编辑窗口中打开。
    pub(crate) fn edit_current_game(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = &self.game_session else {
            return;
        };
        let id = session.entry_id.clone();
        let entry = self
            .settings
            .get()
            .games
            .into_iter()
            .find(|entry| entry.id == id);
        self.spawn_game_editor(entry, window, cx);
    }

    /// 关闭设置页：编辑窗口关闭自身，主窗口恢复原来的浏览位置。
    pub(crate) fn close_game_setup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.game_setup = None;
        if self.game_editor_only {
            window.remove_window();
            return;
        }
        if self.game_session.is_some() {
            self.screen = Screen::Browser;
            set_input(
                &self.location_input,
                self.browser.current_display().as_str(),
                window,
                cx,
            );
        } else {
            self.restore_previous_directory(window, cx);
        }
        cx.notify();
    }

    /// 扫描用户选择的游戏目录。
    pub(crate) fn scan_game_directory(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(setup) = self.game_setup.as_mut() else {
            return;
        };
        set_input(&setup.directory, &path.to_string_lossy(), window, cx);
        setup.busy = true;
        setup.scan_error = None;
        setup.message = None;
        match scan_game(&path) {
            Ok(game) => {
                if setup.name.read(cx).value().trim().is_empty() {
                    set_input(&setup.name, &game.name, window, cx);
                }
                setup.scanned = Some(game);
                setup.busy = false;
                if setup.mappings.is_none()
                    && let Some(candidate) = setup
                        .scanned
                        .as_ref()
                        .and_then(|game| game.mapping_candidates.first())
                        .cloned()
                {
                    setup.mappings = Some(candidate);
                    setup.message = Some(
                        tr(
                            "已自动发现游戏目录中的 .usmap 映射；可手动更换",
                            "Found a .usmap in the game folder; you can replace it",
                        )
                        .to_string(),
                    );
                }
                self.configure_setup_oodle();
            }
            Err(error) => {
                setup.scanned = None;
                setup.scan_error = Some(error.to_string());
                setup.busy = false;
            }
        }
        cx.notify();
    }

    /// 选择游戏目录并立刻扫描。
    pub(crate) fn choose_game_directory(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(tr("选择游戏安装目录", "Choose game install folder").into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(directory) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |app, window, cx| {
                    app.scan_game_directory(directory, window, cx);
                });
            }
        })
        .detach();
    }

    /// 自动尝试游戏目录中的 Oodle 库。
    fn configure_setup_oodle(&mut self) {
        let Some(setup) = self.game_setup.as_mut() else {
            return;
        };
        if setup.oodle.is_some() {
            return;
        }
        let Some(game) = &setup.scanned else {
            return;
        };
        for candidate in &game.oodle_candidates {
            if lumark_archive::configure_oodle_library(Some(candidate.clone())).is_ok() {
                setup.oodle = Some(candidate.clone());
                return;
            }
        }
        let _ = lumark_archive::configure_oodle_library(None);
    }

    /// 用户手动选择 Oodle 动态库。
    pub(crate) fn choose_game_oodle(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr("选择 Oodle 动态库", "Choose Oodle library").into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update(cx, |app, cx| {
                    if let Some(setup) = app.game_setup.as_mut() {
                        match lumark_archive::configure_oodle_library(Some(path.clone())) {
                            Ok(()) => {
                                setup.oodle = Some(path);
                                setup.message =
                                    Some(tr("Oodle 库已加载", "Oodle library loaded").to_string());
                            }
                            Err(error) => {
                                setup.message = Some(format!(
                                    "{}{error}",
                                    tr("无法加载 Oodle 库：", "Cannot load Oodle library: ")
                                ));
                            }
                        }
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    /// 用户选择 `.usmap` 映射文件。
    pub(crate) fn choose_game_mappings(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr("选择 .usmap 映射文件", "Choose .usmap mappings").into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(path) = paths.into_iter().next()
            {
                let is_usmap = path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("usmap"));
                if !is_usmap || !path.is_file() {
                    let _ = this.update(cx, |app, cx| {
                        if let Some(setup) = app.game_setup.as_mut() {
                            setup.message = Some(if is_usmap {
                                tr("映射文件不存在", "Mappings file not found").to_string()
                            } else {
                                tr("映射文件必须是 .usmap", "Mappings must be a .usmap file")
                                    .to_string()
                            });
                            cx.notify();
                        }
                    });
                    return;
                }
                let parse_path = path.clone();
                let _ = this.update(cx, |app, cx| {
                    if let Some(setup) = app.game_setup.as_mut() {
                        setup.message =
                            Some(tr("正在读取映射文件…", "Reading mappings...").to_string());
                        cx.notify();
                    }
                });
                let result = cx
                    .background_executor()
                    .spawn(async move { Mappings::parse_file(&parse_path) })
                    .await;
                let _ = this.update(cx, |app, cx| {
                    if let Some(setup) = app.game_setup.as_mut() {
                        match result {
                            Ok(mappings) => {
                                setup.mappings = Some(path);
                                setup.message = Some(format!(
                                    "{} {} {} / {} {}",
                                    tr("映射已加载：", "Mappings loaded:"),
                                    mappings.structs.len(),
                                    tr("个结构", "structs"),
                                    mappings.enums.len(),
                                    tr("个枚举", "enums")
                                ));
                            }
                            Err(error) => {
                                setup.message = Some(format!(
                                    "{}：{error}",
                                    tr("无法读取映射文件", "Cannot read mappings file")
                                ));
                            }
                        }
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    /// 在后台从 Shipping.exe 提取 AES 密钥。
    pub(crate) fn extract_setup_key(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(setup) = self.game_setup.as_mut() else {
            return;
        };
        if setup.busy || setup.scanned.is_none() {
            return;
        }
        setup.key_state = SetupKeyState::Extracting;
        setup.busy = true;
        setup.message = None;
        let Some(game) = setup.scanned.clone() else {
            setup.busy = false;
            return;
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    extract_key_for_game(&game, None, |_stage: KeyStage, _done, _total| {})
                })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.finish_setup_key(result, cx);
            });
        })
        .detach();
        cx.notify();
    }

    /// 应用后台密钥提取结果。
    fn finish_setup_key(&mut self, result: lumark_unreal::KeyExtraction, cx: &mut Context<Self>) {
        let Some(setup) = self.game_setup.as_mut() else {
            return;
        };
        setup.busy = false;
        match result.key {
            Some(key) => {
                setup.key_state = SetupKeyState::Found(key.clone());
                setup.message = Some(format!(
                    "{}{}",
                    tr(
                        "已提取并验证 AES 密钥：",
                        "AES key extracted and verified: "
                    ),
                    result
                        .validated_against
                        .as_deref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_default()
                ));
            }
            None => {
                let message = result.message.unwrap_or_else(|| {
                    tr("未能提取 AES 密钥", "Cannot extract AES key").to_string()
                });
                setup.key_state = SetupKeyState::Error(message.clone());
                setup.message = Some(message);
            }
        }
        cx.notify();
    }

    /// 在文本/十六进制查看器中查看已提取的 AES 密钥。
    pub(crate) fn view_setup_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(setup) = self.game_setup.as_ref() else {
            return;
        };
        let Some(key) = setup.key_state.key() else {
            self.status = tr("尚无可查看的 AES 密钥", "No AES key to view").to_string();
            cx.notify();
            return;
        };
        let text = format!("{}\n", key.as_str()).into_bytes();
        let bytes = decode_hex_key(key.as_str()).unwrap_or_else(|| text.clone());
        self.open_memory_viewer(
            tr("AES 密钥", "AES key").to_string(),
            tr(
                "内存内容 · 提取的 AES 密钥",
                "In memory · extracted AES key",
            )
            .to_string(),
            text,
            bytes,
            window,
            cx,
        );
    }

    /// 保存设置页并打开游戏。
    pub(crate) fn save_game_setup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(setup) = self.game_setup.as_ref() else {
            return;
        };
        let Some(game) = setup.scanned.clone() else {
            return;
        };
        let name = input_value(&setup.name, cx).trim().to_string();
        if name.is_empty() {
            self.status = tr("请输入游戏名称", "Enter a game name").to_string();
            return;
        }
        // 手动输入优先于已提取密钥；两者都没有时允许保存，打开容器时再提示。
        let manual = input_value(&setup.key_input, cx).trim().to_string();
        let key = if manual.is_empty() {
            setup.key_state.key().cloned()
        } else {
            Some(Zeroizing::new(manual))
        };
        let mut key_source = AesKeySource::None;
        let mut protected_key = None;
        if let Some(key) = &key {
            let encrypted = game
                .containers
                .iter()
                .find(|container| container.encrypted == Some(true));
            if let Some(container) = encrypted
                && !verify_container_key(&container.path, key.as_str())
            {
                self.status = tr(
                    "AES 密钥无法通过容器校验，请检查后重试",
                    "The AES key does not validate against the container",
                )
                .to_string();
                return;
            }
            match platform::secret::protect_text(key) {
                Ok(blob) => {
                    protected_key = Some(blob);
                    key_source = if setup.key_state.key().is_some() {
                        AesKeySource::Extracted
                    } else {
                        AesKeySource::Manual
                    };
                }
                Err(error) => {
                    self.status = format!(
                        "{}{error}",
                        tr(
                            "无法使用 DPAPI 保护密钥：",
                            "Cannot protect the key with DPAPI: "
                        )
                    );
                    return;
                }
            }
        }
        let entry = GameLibraryEntry {
            id: setup
                .editing_id
                .clone()
                .unwrap_or_else(|| Uuid::new_v4().to_string()),
            name,
            root: game.root.clone(),
            engine: setup.engine().setting().to_string(),
            engine_manual: setup.engine_override.is_some(),
            aes_key: protected_key,
            key_source,
            oodle_dll: setup.oodle.clone(),
            mappings: setup.mappings.clone(),
            last_opened: Some(chrono::Local::now().to_rfc3339()),
        };
        let id = entry.id.clone();
        if let Err(error) = self.settings.update(|settings| {
            if let Some(existing) = settings.games.iter_mut().find(|game| game.id == id) {
                *existing = entry.clone();
            } else {
                settings.games.push(entry.clone());
            }
            settings.active_game = Some(id.clone());
        }) {
            self.status = error.to_string();
            return;
        }
        self.game_setup = None;
        if self.game_editor_only {
            self.status = tr("游戏设置已保存", "Game settings saved").to_string();
            window.remove_window();
            return;
        }
        self.open_saved_game(&id, window, cx);
    }

    /// 返回当前打开的游戏（含后台会话），用于侧栏切换。
    #[cfg(test)]
    pub(crate) fn open_game_entries(&self) -> Vec<(String, String, bool)> {
        let mut rows = Vec::new();
        if let Some(session) = &self.game_session {
            rows.push((session.entry_id.clone(), session.game.name.clone(), true));
        }
        for inactive in &self.inactive_games {
            rows.push((
                inactive.session.entry_id.clone(),
                inactive.session.game.name.clone(),
                false,
            ));
        }
        rows
    }

    /// 把当前会话和浏览器状态暂存到后台。
    fn stash_active_game(&mut self) {
        if let Some(session) = self.game_session.take() {
            self.inactive_games.push(InactiveGame {
                session,
                browser: self.browser.clone(),
            });
        }
    }

    /// 把当前打开的游戏 ID 写入设置，供下次启动恢复。
    fn persist_open_games(&self) {
        let mut ids = Vec::new();
        if let Some(session) = &self.game_session {
            ids.push(session.entry_id.clone());
        }
        ids.extend(
            self.inactive_games
                .iter()
                .map(|inactive| inactive.session.entry_id.clone()),
        );
        let _ = self.settings.update(|settings| settings.open_games = ids);
    }

    /// 关闭当前游戏；如果还有后台游戏则切换到下一个。
    pub(crate) fn close_current_game(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.game_session = None;
        self.details_preview = None;
        self.details_mesh = None;
        self.details_material = None;
        self.details_skeleton = None;
        if let Some(next) = self.inactive_games.pop() {
            self.game_session = Some(next.session);
            self.browser = next.browser;
            self.screen = Screen::Browser;
            self.game_library = false;
            if let Some(session) = &self.game_session {
                self.status = format!(
                    "{}：{}",
                    tr("已切换游戏", "Switched game"),
                    session.display_name
                );
            }
            set_input(
                &self.location_input,
                self.browser.current_display().as_str(),
                window,
                cx,
            );
        } else {
            self.game_setup = None;
            self.archive_password = None;
            self.restore_previous_directory(window, cx);
        }
        self.persist_open_games();
        cx.notify();
    }

    /// 切换到已打开的游戏；没有打开时回退到从游戏库加载。
    #[cfg(test)]
    pub(crate) fn switch_game_session(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .game_session
            .as_ref()
            .is_some_and(|session| session.entry_id == id)
        {
            return;
        }
        if let Some(index) = self
            .inactive_games
            .iter()
            .position(|inactive| inactive.session.entry_id == id)
        {
            self.stash_active_game();
            let target = self.inactive_games.remove(index);
            self.game_session = Some(target.session);
            self.browser = target.browser;
            self.details_preview = None;
            self.details_mesh = None;
            self.details_material = None;
            self.details_skeleton = None;
            self.game_setup = None;
            self.screen = Screen::Browser;
            self.game_library = false;
            if let Some(session) = &self.game_session {
                self.status = format!(
                    "{}：{}",
                    tr("已切换游戏", "Switched game"),
                    session.display_name
                );
            }
            set_input(
                &self.location_input,
                self.browser.current_display().as_str(),
                window,
                cx,
            );
            let _ = self.settings.update(|settings| {
                settings.active_game = Some(id.to_string());
                if let Some(game) = settings.games.iter_mut().find(|game| game.id == id) {
                    game.last_opened = Some(chrono::Local::now().to_rfc3339());
                }
            });
            self.persist_open_games();
            cx.notify();
            return;
        }
        self.open_saved_game(id, window, cx);
    }

    /// 打开设置中已保存的游戏。
    pub(crate) fn open_saved_game(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self
            .settings
            .get()
            .games
            .into_iter()
            .find(|entry| entry.id == id)
        else {
            self.status = tr("游戏记录不存在", "Game entry not found").to_string();
            return;
        };
        // 返回列表后再次打开同一个游戏时保留浏览位置，不重新扫描目录。
        if self
            .game_session
            .as_ref()
            .is_some_and(|session| session.entry_id == id)
        {
            if let Some(session) = self.game_session.as_mut() {
                session.display_name = entry.name.clone();
            }
            self.game_library = false;
            self.screen = Screen::Browser;
            set_input(
                &self.location_input,
                self.browser.current_display().as_str(),
                window,
                cx,
            );
            cx.notify();
            return;
        }
        // 重新加载同一游戏配置时替换旧实例，避免出现两个同名会话。
        self.inactive_games
            .retain(|inactive| inactive.session.entry_id != id);
        let game = match scan_game(&entry.root) {
            Ok(game) => game,
            Err(error) => {
                self.status = error.to_string();
                return;
            }
        };
        let key = entry
            .aes_key
            .as_deref()
            .and_then(|blob| platform::secret::unprotect_text(blob).ok())
            .map(Zeroizing::new);
        let encrypted_container = game
            .containers
            .iter()
            .find(|container| container.encrypted == Some(true));
        let key_state = match (&key, encrypted_container) {
            (_, None) => GameKeyState::NotNeeded,
            (Some(key), Some(container)) => {
                if verify_container_key(&container.path, key.as_str()) {
                    GameKeyState::Valid
                } else {
                    GameKeyState::Invalid
                }
            }
            (None, Some(_)) => GameKeyState::Missing,
        };
        let key = if key_state == GameKeyState::Invalid {
            None
        } else {
            key
        };
        let oodle = entry.oodle_dll.clone().or_else(|| {
            game.oodle_candidates.iter().find_map(|candidate| {
                lumark_archive::configure_oodle_library(Some(candidate.clone()))
                    .ok()
                    .map(|()| candidate.clone())
            })
        });
        if oodle.is_none() {
            let _ = lumark_archive::configure_oodle_library(None);
        }
        if encrypted_container.is_some() && key.is_none() {
            self.status = tr(
                "游戏容器已加密；请编辑游戏并提取或输入 AES 密钥",
                "Game containers are encrypted; edit the game to provide an AES key",
            )
            .to_string();
        }
        let session = GameSession {
            entry_id: entry.id.clone(),
            display_name: entry.name.clone(),
            key,
            key_state,
            oodle,
            mappings: entry.mappings.clone(),
            last_container: None,
            game,
        };
        if self.game_previous_dir.is_none() {
            self.remember_previous_directory();
        }
        // 同一个游戏重新加载时替换当前实例；打开另一个游戏时把当前
        // 会话和浏览历史暂存到后台，实现多游戏同时打开、互不影响。
        if self
            .game_session
            .as_ref()
            .is_some_and(|active| active.entry_id == id)
        {
            self.game_session = None;
        } else {
            self.stash_active_game();
        }
        self.browser
            .show_game_archives(session.display_name.clone(), session.rows());
        self.game_session = Some(session);
        self.details_preview = None;
        self.details_mesh = None;
        self.details_material = None;
        self.details_skeleton = None;
        self.game_setup = None;
        self.screen = Screen::Browser;
        self.game_library = false;
        set_input(
            &self.location_input,
            self.browser.current_display().as_str(),
            window,
            cx,
        );
        let _ = self.settings.update(|settings| {
            settings.active_game = Some(id.to_string());
            if let Some(game) = settings.games.iter_mut().find(|game| game.id == id) {
                game.last_opened = Some(chrono::Local::now().to_rfc3339());
            }
        });
        self.persist_open_games();
        cx.notify();
    }

    /// 确认后从游戏库移除游戏记录。
    pub(crate) fn remove_saved_game(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = self
            .settings
            .get()
            .games
            .into_iter()
            .find(|game| game.id == id)
            .map(|game| game.name)
            .unwrap_or_else(|| tr("该游戏", "this game").to_string());
        let app = cx.entity();
        let id = id.to_string();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let app = app.clone();
            let id = id.clone();
            alert
                .title(format!("{}{name}？", tr("移除 ", "Remove ")))
                .description(tr(
                    "只移除 Lumark 中的游戏记录，不会删除游戏文件或 Oodle 库。",
                    "Only the Lumark game entry is removed; game files and Oodle libraries are untouched.",
                ))
                .confirm()
                .ok_text(tr("移除", "Remove"))
                .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                .cancel_text(tr("取消", "Cancel"))
                .on_ok(move |_, _window, cx| {
                    let app = app.clone();
                    let id = id.clone();
                    app.update(cx, |app, cx| {
                        let current = app
                            .game_session
                            .as_ref()
                            .is_some_and(|session| session.entry_id == id);
                        app.inactive_games
                            .retain(|inactive| inactive.session.entry_id != id);
                        let _ = app.settings.update(|settings| {
                            settings.games.retain(|game| game.id != id);
                            if settings.active_game.as_deref() == Some(id.as_str()) {
                                settings.active_game = None;
                            }
                        });
                        if current {
                            app.game_session = None;
                            app.game_setup = None;
                            app.browser.selection.clear();
                        }
                        app.persist_open_games();
                        cx.notify();
                    });
                    true
                })
        });
    }

    /// 退出游戏资产并通过恢复文件系统位置关闭工作区。
    pub(crate) fn exit_game_assets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.persist_open_games();
        self.game_session = None;
        self.inactive_games.clear();
        self.game_setup = None;
        self.game_library = false;
        self.archive_password = None;
        self.restore_previous_directory(window, cx);
        cx.notify();
    }

    /// 恢复进入游戏资产前的文件系统目录。
    fn restore_previous_directory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(directory) = self.game_previous_dir.take()
            && directory.is_dir()
        {
            let _ = self.browser.navigate(directory);
        }
        set_input(
            &self.location_input,
            self.browser.current_display().as_str(),
            window,
            cx,
        );
    }

    /// 打开游戏归档列表中的容器。
    pub(crate) fn open_game_container(
        &mut self,
        entry: BrowserEntry,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = &self.game_session else {
            return;
        };
        let Some(container) = session
            .game
            .containers
            .iter()
            .find(|container| container.path == entry.path)
        else {
            self.status =
                tr("容器不属于当前游戏", "Container is not part of this game").to_string();
            return;
        };
        let encrypted = container.encrypted == Some(true);
        let password = session.key.clone();
        if encrypted && password.is_none() {
            self.status = tr(
                "该容器已加密，请先提取或输入 AES 密钥",
                "This container is encrypted; extract or enter an AES key first",
            )
            .to_string();
            return;
        }
        let path = container.path.clone();
        self.begin_archive_load(path.clone(), password, ArchiveOpenIntent::Browse);
        if let Some(session) = self.game_session.as_mut() {
            session.last_container = Some(path);
        }
        cx.notify();
    }

    /// 按容器路径打开归档列表中的容器。
    pub(crate) fn open_game_container_path(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(entry) = self
            .browser
            .entries
            .iter()
            .find(|entry| entry.path == path)
            .cloned()
        {
            self.open_game_container(entry, window, cx);
        }
    }

    /// 在游戏容器根目录返回归档列表；返回是否已切换位置。
    pub(crate) fn leave_game_container_to_archives(&mut self) -> bool {
        let Some(session) = &self.game_session else {
            return false;
        };
        let BrowserLocation::Archive { archive, inner } = &self.browser.location else {
            return false;
        };
        if !inner.is_empty() || !session.contains_container(archive) {
            return false;
        }
        let title = session.display_name.clone();
        let rows = session.rows();
        self.browser.show_game_archives(title, rows);
        self.archive_password = None;
        true
    }
}

/// 把 64 位十六进制 AES 密钥解码为 32 字节，供十六进制视图展示。
fn decode_hex_key(key: &str) -> Option<Vec<u8>> {
    let key = key.trim();
    if key.len() != 64 || !key.chars().all(|character| character.is_ascii_hexdigit()) {
        return None;
    }
    key.as_bytes()
        .chunks(2)
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
        })
        .collect()
}
