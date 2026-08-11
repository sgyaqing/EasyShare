use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
}

/// Detect UI language from the Accept-Language header: Chinese wins only
/// when the top q-weighted tag starts with "zh"; everything else is English.
pub fn detect(accept_language: Option<&str>) -> Lang {
    let Some(header) = accept_language else {
        return Lang::En;
    };
    let mut best: Option<(&str, f32)> = None;
    for part in header.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let mut tag = part;
        let mut q = 1.0f32;
        if let Some((t, params)) = part.split_once(';') {
            tag = t.trim();
            for p in params.split(';') {
                let p = p.trim();
                if let Some(v) = p.strip_prefix("q=") {
                    q = v.trim().parse().unwrap_or(0.0);
                }
            }
        }
        if tag.is_empty() {
            continue;
        }
        match best {
            None => best = Some((tag, q)),
            Some((_, bq)) if q > bq => best = Some((tag, q)),
            _ => {}
        }
    }
    match best {
        Some((tag, _)) if tag.to_ascii_lowercase().starts_with("zh") => Lang::Zh,
        _ => Lang::En,
    }
}

#[derive(Debug, Serialize)]
pub struct Strings {
    pub send: &'static str,
    pub input_placeholder: &'static str,
    pub copy: &'static str,
    pub download: &'static str,
    pub recall: &'static str,
    pub recalled: &'static str,
    pub no_more: &'static str,
    pub drop_hint: &'static str,
    pub send_failed: &'static str,
    pub upload_failed: &'static str,
    pub file_too_large: &'static str,
    pub uploading: &'static str,
    pub recall_failed: &'static str,
    pub copy_done: &'static str,
    pub empty_message: &'static str,
}

const ZH: Strings = Strings {
    send: "发送",
    input_placeholder: "输入消息，Enter 发送，Shift+Enter 换行",
    copy: "复制",
    download: "下载",
    recall: "撤回",
    recalled: "撤回了一条消息",
    no_more: "没有更多消息了",
    drop_hint: "松开鼠标发送文件",
    send_failed: "发送失败，请重试",
    upload_failed: "上传失败，请重试",
    file_too_large: "文件超过大小限制（最大 1GB）",
    uploading: "正在上传…",
    recall_failed: "撤回失败",
    copy_done: "已复制",
    empty_message: "不能发送空消息",
};

const EN: Strings = Strings {
    send: "Send",
    input_placeholder: "Type a message. Enter to send, Shift+Enter for newline",
    copy: "Copy",
    download: "Download",
    recall: "Recall",
    recalled: "recalled a message",
    no_more: "No more messages",
    drop_hint: "Drop to send file",
    send_failed: "Failed to send, please retry",
    upload_failed: "Failed to upload, please retry",
    file_too_large: "File exceeds the size limit (max 1 GB)",
    uploading: "Uploading…",
    recall_failed: "Failed to recall",
    copy_done: "Copied",
    empty_message: "Cannot send an empty message",
};

pub fn strings(lang: Lang) -> &'static Strings {
    match lang {
        Lang::Zh => &ZH,
        Lang::En => &EN,
    }
}

pub fn code(lang: Lang) -> &'static str {
    match lang {
        Lang::Zh => "zh",
        Lang::En => "en",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_chinese() {
        assert_eq!(detect(Some("zh-CN,zh;q=0.9,en;q=0.8")), Lang::Zh);
        assert_eq!(detect(Some("zh")), Lang::Zh);
        assert_eq!(detect(Some("zh-TW")), Lang::Zh);
    }

    #[test]
    fn falls_back_to_english() {
        assert_eq!(detect(Some("en-US,en;q=0.9")), Lang::En);
        assert_eq!(detect(Some("fr-FR")), Lang::En);
        assert_eq!(detect(None), Lang::En);
        assert_eq!(detect(Some("")), Lang::En);
    }

    #[test]
    fn respects_q_values() {
        // English preferred over Chinese by q weight.
        assert_eq!(detect(Some("zh;q=0.3,en;q=0.9")), Lang::En);
        // Chinese top-weighted even when listed second.
        assert_eq!(detect(Some("en;q=0.5,zh-CN;q=0.9")), Lang::Zh);
    }

    #[test]
    fn wildcard_is_english() {
        assert_eq!(detect(Some("*")), Lang::En);
    }
}
