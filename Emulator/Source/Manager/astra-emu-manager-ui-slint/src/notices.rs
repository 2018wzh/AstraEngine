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
        "ASTRA_EMU_CORES_NOT_READY" => ("模拟器核心尚未加载", "请先在“核心”页加载核心。"),
        "ASTRA_EMU_FAMILY_INITIALIZING" => ("核心仍在加载或扫描中", "请稍候再试。"),
        "ASTRA_EMU_FAMILY_SESSION_ALREADY_ACTIVE" => {
            ("已有游戏正在运行", "请先返回游戏库结束当前游戏。")
        }
        "ASTRA_EMU_FAMILY_PROBE_REQUIRED" => ("这部作品尚未识别", "请重新扫描游戏目录。"),
        "ASTRA_EMU_FAMILY_PROVIDER_SELECTION_REQUIRED" => (
            "需要选择模拟器",
            "有多个核心可以运行这部作品，请在作品详情的“选项”中选择一个。",
        ),
        "ASTRA_EMU_GAME_NOT_FOUND" => ("找不到这部作品", "它可能已被移除，请重新扫描。"),
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
        "ASTRA_EMU_INPUT_QUEUE_FULL" => ("输入过多", "游戏未能及时处理输入，已停止运行。"),
        "ASTRA_EMU_TRANSLATION_PROFILE_REQUIRED" => {
            ("尚未配置翻译服务", "请在 设置 › 实时翻译 中填写。")
        }
        "ASTRA_EMU_CORE_INSTALL_NAME" => (
            "不是有效的核心文件",
            "核心文件名需以 astra_emu_ 开头，并且是本系统的动态库。",
        ),
        "ASTRA_EMU_CORE_INSTALL_COPY" => ("复制核心文件失败", "请检查磁盘空间和目录权限。"),
        "ASTRA_EMU_CORE_UNINSTALL_BUNDLED" => ("随包核心不能卸载", "可以安装同名文件来覆盖它。"),
        "ASTRA_EMU_CORE_PENDING_APPLY" => ("应用待安装的核心失败", "请查看日志后重试。"),
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
        "HOST" | "WINDOW" | "GPU" | "D3D12" | "FRAME" | "SLINT" | "SYSTEM" | "OUT" => {
            "窗口或显示出错"
        }
        "TEXT" | "TRANSLATION" | "EXTENSION" => "实时翻译出错",
        "METADATA" | "VNDB" | "BANGUMI" => "作品资料服务出错",
        "COMPATIBILITY" => "兼容性数据出错",
        "FILTER" | "EFFECT" | "DXC" => "画面滤镜出错",
        "PATCH" => "文件补丁处理失败",
        "INPUT" | "GAMEPAD" => "输入设备出错",
        "LIBRARY" | "GAME" | "CASE" | "SEARCH" | "DATA" | "EXECUTABLE" => "游戏库出错",
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
