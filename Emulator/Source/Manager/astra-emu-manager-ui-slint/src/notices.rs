use std::{cell::RefCell, collections::HashSet};

use slint::{ComponentHandle, Model, ModelTracker, VecModel};

use crate::{ManagerWindow, Notice, NoticeViewModel, Notices};

pub struct ErrorText {
    pub title: String,
    pub detail: String,
    pub code: String,
}

pub fn describe_error(raw: &str) -> ErrorText {
    let (code, rest) = split_code(raw);
    if code.is_empty() {
        let mut lines = raw.split('\n');
        let title = lines.next().unwrap_or("").to_string();
        let detail = lines.collect::<Vec<_>>().join("\n");
        return ErrorText {
            title,
            detail,
            code: String::new(),
        };
    }
    if let Some((title, detail)) = exact_text(&code) {
        return ErrorText {
            title: title.to_string(),
            detail: detail.to_string(),
            code,
        };
    }
    ErrorText {
        title: fallback_title(&code).to_string(),
        detail: rest,
        code,
    }
}

fn split_code(raw: &str) -> (String, String) {
    let token = raw
        .split(|c: char| c.is_whitespace() || c == ':')
        .next()
        .unwrap_or("");
    if !is_error_code(token) {
        return (String::new(), String::new());
    }
    let rest = raw
        .get(token.len()..)
        .unwrap_or("")
        .strip_prefix(": ")
        .unwrap_or("")
        .to_string();
    (token.to_string(), rest)
}

fn is_error_code(token: &str) -> bool {
    let Some(rest) = token.strip_prefix("ASTRA_EMU_") else {
        return false;
    };
    !rest.is_empty()
        && rest
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

fn exact_text(code: &str) -> Option<(&'static str, &'static str)> {
    Some(match code {
        "ASTRA_EMU_CORES_NOT_READY" => ("模拟器核心尚未加载", "请先在游戏库点击“准备”。"),
        "ASTRA_EMU_FAMILY_INITIALIZING" => ("核心仍在加载或扫描中", "请稍候再试。"),
        "ASTRA_EMU_PREPARE_BUSY" => ("正在准备资料库", "请等待当前的核心加载或扫描结束。"),
        "ASTRA_EMU_PREPARE_NOT_CANCELLABLE" => ("当前没有可取消的扫描", "核心加载阶段不能取消。"),
        "ASTRA_EMU_PREPARE_RESTART_REQUIRED" => {
            ("资料库准备失败", "请查看核心诊断，修复后重启 AstraEMU。")
        }
        "ASTRA_EMU_SCAN_CANCELLED" => ("已取消扫描", "资料库保持不变。"),
        "ASTRA_EMU_GAME_DIRECTORY_INVALID" => ("无法使用这个目录", "目录不存在或无法访问。"),
        "ASTRA_EMU_UNDO_UNAVAILABLE" => ("没有可撤销的移除", "作品可以在 设置→库目录 中恢复。"),
        "ASTRA_EMU_OPEN_PATH_FAILED" => ("无法打开所在目录", "系统文件管理器未能启动。"),
        "ASTRA_EMU_KEY_CAPTURE_INACTIVE" => ("没有正在设置的按键", "请先点选要绑定的手柄按钮。"),
        "ASTRA_EMU_PATCH_RECIPE_UNKNOWN" => (
            "补丁目录中没有这个补丁",
            "补丁列表可能已随元数据同步更新，请重新选择。",
        ),
        "ASTRA_EMU_GAMEPAD_BUTTON_INVALID" => ("无法识别的手柄按钮", "请从按键列表中选择。"),
        "ASTRA_EMU_FAMILY_SESSION_ALREADY_ACTIVE" => {
            ("已有游戏正在运行", "请先返回游戏库结束当前游戏。")
        }
        "ASTRA_EMU_FAMILY_PROBE_REQUIRED" => ("这部作品尚未识别", "请重新扫描游戏目录。"),
        "ASTRA_EMU_FAMILY_PROVIDER_SELECTION_REQUIRED" => (
            "需要选择模拟器",
            "有多个模拟器可以运行这部作品，请在作品详情中选择一个。",
        ),
        "ASTRA_EMU_GAME_NOT_FOUND" => ("找不到这部作品", "它可能已被移除，请刷新资料库。"),
        "ASTRA_EMU_OWNER_ALREADY_STARTED" => ("核心已加载", "更换核心后需要重启 AstraEMU。"),
        "ASTRA_EMU_OWNER_PANICKED" => ("核心后台意外退出", "请重启 AstraEMU 并查看日志。"),
        "ASTRA_EMU_FAMILY_LOAD_LIBRARY" => {
            ("无法加载核心文件", "文件可能已损坏，或缺少它依赖的运行库。")
        }
        "ASTRA_EMU_FAMILY_LOAD_ABI" => {
            ("核心版本与 AstraEMU 不兼容", "请安装与当前版本匹配的核心。")
        }
        "ASTRA_EMU_FAMILY_LOAD_DESCRIPTOR" => ("核心信息无效", "核心文件可能已损坏。"),
        "ASTRA_EMU_FAMILY_LOAD_DUPLICATE_PLUGIN" => (
            "发现重复的核心",
            "同一个核心存在多份文件，已全部停用。删除多余的一份后重启。",
        ),
        "ASTRA_EMU_FAMILY_LOAD_PATH" => ("核心文件位置无效", "请检查核心文件是否仍然存在。"),
        "ASTRA_EMU_FAMILY_LOAD_POLICY" => ("核心不符合安全要求", "已停用这个核心。"),
        "ASTRA_EMU_FAMILY_DIAGNOSTIC_INIT" => ("核心诊断初始化失败", "请查看日志。"),
        "ASTRA_EMU_FAMILY_SCAN_DIRECTORY" => ("无法读取核心目录", "请检查目录权限。"),
        "ASTRA_EMU_CORE_RETRY_UNAVAILABLE" => ("无法重试", "这个错误不支持重试，请结束游戏。"),
        "ASTRA_EMU_INPUT_QUEUE_FULL" => ("输入过多", "游戏未能及时处理输入，多余的输入已丢弃。"),
        "ASTRA_EMU_RENDER_PREFERENCE_INVALID" => (
            "渲染分辨率设置无效",
            "预设只能选 720p、1080p、1440p 或 2160p，自定义宽高须在 1 到 16384 之间。",
        ),
        "ASTRA_EMU_RENDER_EXTENT_UNSUPPORTED" => (
            "自定义分辨率超出该模拟器支持范围",
            "请在游戏设置中查看该模拟器支持的分辨率范围。",
        ),
        "ASTRA_EMU_EMULATOR_SELECTION_REQUIRED" => (
            "需要先选择模拟器",
            "选定模拟器后才能修改这部作品的模拟器设置。",
        ),
        "ASTRA_EMU_CONFIG_PATH_INVALID" => ("无法使用所选路径", "文件或目录不存在或无法访问。"),
        "ASTRA_EMU_CONFIG_PATH_UTF8" => (
            "所选路径包含无法保存的字符",
            "请把文件放到路径只含常规字符的位置后重新选择。",
        ),
        "ASTRA_EMU_FAMILY_CONFIG" => ("设置值无效", "取值不在允许范围内，已保留原来的设置。"),
        "ASTRA_EMU_RELEASE_ID_INVALID" => (
            "发行版本编号无效",
            "VNDB 发行版本编号应为 r 加数字，例如 r12345，且不超过 64 个字符。",
        ),
        "ASTRA_EMU_COMPATIBILITY_DUPLICATE" => (
            "兼容性数据有重复条目",
            "同一发行版本和模拟器出现了多条记录，本次更新未采用。",
        ),
        "ASTRA_EMU_TRANSLATION_PROFILE_REQUIRED" => {
            ("尚未配置翻译服务", "请在 设置→实时翻译 中填写并保存。")
        }
        "ASTRA_EMU_NETWORK_PERMISSION_REQUIRED" => {
            ("尚未允许联网", "需要先在 设置→联网与资料 中允许联网")
        }
        "ASTRA_EMU_BANGUMI_TOKEN_REQUIRED" => ("请填写 Bangumi 访问令牌", "令牌为空时无法保存。"),
        "ASTRA_EMU_CORE_INSTALL_NAME" => (
            "不是有效的核心文件",
            "请选择核心包（.zip）或文件名以 astra_emu_ 开头的本系统动态库。",
        ),
        "ASTRA_EMU_CORE_INSTALL_COPY" => ("复制核心文件失败", "请检查磁盘空间和目录权限。"),
        "ASTRA_EMU_CORE_UNINSTALL_BUNDLED" => ("随包核心不能卸载", "可以安装同名核心来覆盖它。"),
        "ASTRA_EMU_CORE_PENDING_APPLY" => ("应用待安装的核心失败", "请查看日志后重试。"),
        "ASTRA_EMU_CORE_LAYOUT" => (
            "核心目录结构不正确",
            "cores 下每个核心目录必须恰好包含一个 astra_emu_ 核心库，请重新安装该核心。",
        ),
        "ASTRA_EMU_CORE_PACKAGE_READ" => ("无法读取核心包", "请检查文件是否存在且可以访问。"),
        "ASTRA_EMU_CORE_PACKAGE_FORMAT" => ("核心包已损坏", "文件不是有效的 zip 核心包。"),
        "ASTRA_EMU_CORE_PACKAGE_BOUNDS" => ("核心包过大", "核心包的文件数或解压后大小超出上限。"),
        "ASTRA_EMU_CORE_PACKAGE_PATH" => (
            "核心包包含不安全的路径",
            "包内有绝对路径、上级目录或符号链接，已拒绝安装。",
        ),
        "ASTRA_EMU_CORE_PACKAGE_DUPLICATE" => ("核心包包含重复文件", "已拒绝安装这个核心包。"),
        "ASTRA_EMU_CORE_PACKAGE_MANIFEST" => (
            "核心包的文件清单无效",
            "core.json 缺失、格式错误、ABI 不匹配或与包内文件不一致。",
        ),
        "ASTRA_EMU_CORE_PACKAGE_PLUGIN_MISMATCH" => (
            "核心包与所选核心不符",
            "下载的核心包声明了另一个模拟器，已拒绝安装。",
        ),
        "ASTRA_EMU_CORE_PACKAGE_HASH_MISMATCH" => (
            "核心包文件校验失败",
            "包内文件的 SHA-256 与 core.json 不一致，已拒绝安装。",
        ),
        "ASTRA_EMU_CORE_NOT_AVAILABLE" => (
            "没有可安装的核心版本",
            "元数据中没有适用于当前平台和接口版本的发布，请先同步元数据。",
        ),
        "ASTRA_EMU_CORE_DOWNLOAD_PENDING" => ("核心正在下载", "请等待当前下载完成。"),
        "ASTRA_EMU_CORE_DOWNLOAD_URL" => {
            ("核心下载地址无效", "只允许从 GitHub Releases 下载核心。")
        }
        "ASTRA_EMU_CORE_DOWNLOAD_REDIRECT" => (
            "核心下载被重定向到未知地址",
            "只允许跳转到 GitHub 的发布文件服务器，已停止下载。",
        ),
        "ASTRA_EMU_CORE_DOWNLOAD_NETWORK" => ("核心下载失败", "请检查网络连接后重试。"),
        "ASTRA_EMU_CORE_DOWNLOAD_SIZE" => (
            "核心包大小不符",
            "下载的文件大小与元数据记录不一致，已丢弃。",
        ),
        "ASTRA_EMU_CORE_DOWNLOAD_HASH_MISMATCH" => (
            "核心包校验失败",
            "下载文件的 SHA-256 与元数据记录不一致，已丢弃。",
        ),
        "ASTRA_EMU_SNACKBAR_ACTION_UNAVAILABLE" => ("这个操作已失效", "提示已过期，请重新操作。"),
        "ASTRA_EMU_METADATA_HASH_MISMATCH" => (
            "元数据校验失败",
            "下载的文件与索引记录不一致，已保留原有数据。",
        ),
        "ASTRA_EMU_METADATA_CACHE_WRITE" => (
            "无法保存元数据",
            "写入数据目录失败，已保留原有数据。请检查磁盘空间和目录权限。",
        ),
        "ASTRA_EMU_METADATA_CACHE_READ" => ("无法读取元数据缓存", "请检查数据目录权限。"),
        "ASTRA_EMU_METADATA_CACHE_INVALID" => {
            ("元数据内容无效", "同步到的数据无法解析，已保留原有数据。")
        }
        "ASTRA_EMU_METADATA_REQUEST_REJECTED" => (
            "作品资料服务拒绝了请求",
            "VNDB 不接受这次查询。请更新 AstraEMU 后重试，仍失败请反馈问题。",
        ),
        "ASTRA_EMU_METADATA_RATE_LIMITED" => (
            "作品资料请求过于频繁",
            "VNDB 或 Bangumi 暂时限制了请求，请稍后再试。",
        ),
        "ASTRA_EMU_PRESET_INVALID" => (
            "推荐配置无法套用",
            "其中有当前模拟器不支持的设置项或取值，已恢复原来的设置。",
        ),
        "ASTRA_EMU_PRESET_UNAVAILABLE" => ("没有推荐配置", "当前发行版本和模拟器没有推荐配置。"),
        "ASTRA_EMU_LOG_READ" => ("无法读取日志", "请检查数据目录权限。"),
        "ASTRA_EMU_ENGINE_ACTION_UNAVAILABLE" => {
            ("当前无法执行这个操作", "请在游戏画面中稍后再试。")
        }
        _ => return None,
    })
}

fn fallback_title(code: &str) -> &'static str {
    let rest = code.strip_prefix("ASTRA_EMU_").unwrap_or(code);
    match rest.split('_').next().unwrap_or("") {
        "FAMILY" | "CORE" | "CORES" | "MODULE" | "OWNER" | "PROBE" | "EMULATOR" | "STATIC"
        | "MUSICA" | "FVP" | "SESSION" => "模拟器核心出错",
        "HOST" | "WINDOW" | "GPU" | "FRAME" | "SLINT" | "SYSTEM" | "OUT" | "RENDER" => {
            "窗口或显示出错"
        }
        "TEXT" | "TRANSLATION" | "EXTENSION" => "实时翻译出错",
        "METADATA" | "VNDB" | "BANGUMI" | "NETWORK" | "RELEASE" => "作品资料服务出错",
        "COMPATIBILITY" => "兼容性数据出错",
        "FILTER" | "EFFECT" | "DXC" => "画面滤镜出错",
        "PATCH" => "文件补丁处理失败",
        "INPUT" | "GAMEPAD" => "输入设备出错",
        "LIBRARY" | "GAME" | "CASE" | "SEARCH" | "DATA" | "EXECUTABLE" | "PREPARE" | "SCAN"
        | "OPEN" => "游戏库出错",
        "PRESET" => "推荐配置出错",
        "AUDIO" => "声音输出出错",
        "MODAL" | "MANAGER" => "管理器出错",
        _ => "发生错误",
    }
}

pub(crate) struct NoticeModel {
    rows: VecModel<Notice>,
    model_items: RefCell<Vec<NoticeViewModel>>,
    host_items: RefCell<Vec<NoticeViewModel>>,
    dismissed: RefCell<HashSet<String>>,
}

impl NoticeModel {
    pub(crate) fn new() -> Self {
        Self {
            rows: VecModel::default(),
            model_items: RefCell::new(Vec::new()),
            host_items: RefCell::new(Vec::new()),
            dismissed: RefCell::new(HashSet::new()),
        }
    }

    pub(crate) fn set_model(&self, items: Vec<NoticeViewModel>) {
        let mut dismissed = self.dismissed.borrow_mut();
        let host = self.host_items.borrow();
        dismissed.retain(|id| {
            items.iter().any(|item| item.id == *id) || host.iter().any(|item| item.id == *id)
        });
        drop(host);
        *self.model_items.borrow_mut() = items;
        drop(dismissed);
        self.rebuild();
    }

    pub(crate) fn report(&self, raw: &str) {
        let text = describe_error(raw);
        let id = format!(
            "host:{}",
            if text.code.is_empty() {
                text.title.as_str()
            } else {
                text.code.as_str()
            }
        );
        let mut host = self.host_items.borrow_mut();
        host.retain(|item| item.id != id);
        host.insert(
            0,
            NoticeViewModel {
                id: id.clone(),
                severity: "error".into(),
                title: text.title,
                detail: text.detail,
                code: text.code,
                core: String::new(),
            },
        );
        host.truncate(5);
        drop(host);
        self.dismissed.borrow_mut().remove(&id);
        self.rebuild();
    }

    pub(crate) fn dismiss(&self, id: &str) {
        self.dismissed.borrow_mut().insert(id.to_string());
        self.host_items.borrow_mut().retain(|item| item.id != id);
        self.rebuild();
    }

    fn rebuild(&self) {
        let dismissed = self.dismissed.borrow();
        let mut rows = Vec::new();
        for item in self.host_items.borrow().iter() {
            if !dismissed.contains(&item.id) {
                rows.push(to_notice(item));
            }
        }
        for item in self.model_items.borrow().iter() {
            if !dismissed.contains(&item.id) {
                rows.push(to_notice(item));
            }
        }
        self.rows.set_vec(rows);
    }
}

fn to_notice(item: &NoticeViewModel) -> Notice {
    Notice {
        id: item.id.as_str().into(),
        severity: item.severity.as_str().into(),
        title: item.title.as_str().into(),
        detail: item.detail.as_str().into(),
        code: item.code.as_str().into(),
        core: item.core.as_str().into(),
    }
}

impl Model for NoticeModel {
    type Data = Notice;
    fn row_count(&self) -> usize {
        self.rows.row_count()
    }
    fn row_data(&self, row: usize) -> Option<Notice> {
        self.rows.row_data(row)
    }
    fn model_tracker(&self) -> &dyn ModelTracker {
        self.rows.model_tracker()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub fn report_error(window: &ManagerWindow, raw: &str) {
    let items = window.global::<Notices>().get_items();
    if let Some(model) = items.as_any().downcast_ref::<NoticeModel>() {
        model.report(raw);
        return;
    }
    tracing::error!(event = "astra.emu.manager.notice_model_missing", raw);
}

#[cfg(test)]
mod tests {
    use super::describe_error;

    #[test]
    fn describe_error_falls_back_by_family_token() {
        let text = describe_error("ASTRA_EMU_FILTER_FOO: bad");
        assert_eq!(text.detail, "bad");
        assert_eq!(text.code, "ASTRA_EMU_FILTER_FOO");
    }

    #[test]
    fn describe_error_uses_first_line_without_code() {
        let text = describe_error("扫描失败\n原因");
        assert_eq!(text.title, "扫描失败");
        assert_eq!(text.detail, "原因");
        assert_eq!(text.code, "");
    }
}
