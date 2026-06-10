/// Askama-based prompt template engine.
///
/// Renders Jinja2 templates into system/user prompt pairs for LLM inference.
/// Templates are compiled at Rust compile time — zero filesystem I/O at runtime.
///
/// Adapted from NR Log's template-app crate.

use std::cell::RefCell;

use askama::Template;

// ─── Askama Filters (askama 0.12 style: plain functions in `mod filters`) ──

mod filters {
    use super::*;

    pub fn current_date<T: ?Sized>(
        _value: &T,
    ) -> askama::Result<String> {
        CURRENT_DATE_OVERRIDE.with(|v| {
            if let Some(ref date) = *v.borrow() {
                return Ok(date.clone());
            }
            Ok(chrono::Utc::now().format("%Y-%m-%d").to_string())
        })
    }

    pub fn language(value: &Option<String>) -> askama::Result<String> {
        let raw = value.as_deref().unwrap_or("").to_lowercase();
        let v = extract_iso639(&raw);
        match v {
            "en" => Ok("English".to_string()),
            "ko" => Ok("Korean".to_string()),
            "ja" => Ok("Japanese".to_string()),
            "zh" => Ok("Chinese".to_string()),
            "fr" => Ok("French".to_string()),
            "de" => Ok("German".to_string()),
            "es" => Ok("Spanish".to_string()),
            "pt" => Ok("Portuguese".to_string()),
            "ru" => Ok("Russian".to_string()),
            "it" => Ok("Italian".to_string()),
            "ar" => Ok("Arabic".to_string()),
            "hi" => Ok("Hindi".to_string()),
            _ => Ok("English".to_string()),
        }
    }

    pub fn is_english(value: &Option<String>) -> askama::Result<bool> {
        let raw = value.as_deref().unwrap_or("en").to_lowercase();
        let v = extract_iso639(&raw);
        Ok(v == "en")
    }

    pub fn is_korean(value: &Option<String>) -> askama::Result<bool> {
        let raw = value.as_deref().unwrap_or("en").to_lowercase();
        let v = extract_iso639(&raw);
        Ok(v == "ko")
    }
}

thread_local! {
    static CURRENT_DATE_OVERRIDE: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub fn set_current_date_override(date: Option<String>) {
    CURRENT_DATE_OVERRIDE.with(|v| *v.borrow_mut() = date);
}

fn extract_iso639(code: &str) -> &str {
    code.split(['-', '_']).next().unwrap_or(code)
}

// ─── Template Data Types ────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Segment {
    pub text: String,
    pub speaker: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Transcript {
    pub segments: Vec<Segment>,
    pub started_at: Option<u64>,
    pub ended_at: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Event {
    pub name: String,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Session {
    pub title: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub event: Option<Event>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Participant {
    pub name: String,
    pub job_title: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TemplateSection {
    pub title: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EnhanceTemplate {
    pub title: String,
    pub description: Option<String>,
    pub sections: Vec<TemplateSection>,
}

// ─── System Prompt Templates ────────────────────────────────────────────────

#[derive(Template)]
#[template(path = "enhance.system.md.jinja")]
pub struct EnhanceSystem {
    pub language_name: String,
    pub is_english: bool,
    pub is_korean: bool,
    pub current_date: String,
}

impl EnhanceSystem {
    pub fn with_language(language: Option<String>) -> Self {
        let raw = language.as_deref().unwrap_or("").to_lowercase();
        let code = extract_iso639(&raw);
        let language_name = match code {
            "en" => "English".to_string(),
            "ko" => "Korean".to_string(),
            "ja" => "Japanese".to_string(),
            "zh" => "Chinese".to_string(),
            "fr" => "French".to_string(),
            "de" => "German".to_string(),
            "es" => "Spanish".to_string(),
            _ => "English".to_string(),
        };
        Self {
            language_name,
            is_english: code == "en",
            is_korean: code == "ko",
            current_date: chrono::Utc::now().format("%Y-%m-%d").to_string(),
        }
    }
}

#[derive(Template)]
#[template(path = "title.system.md.jinja")]
pub struct TitleSystem {
    pub language_name: String,
    pub current_date: String,
}

#[derive(Template)]
#[template(path = "chat.system.md.jinja")]
pub struct ChatSystem {
    pub language_name: String,
    pub current_date: String,
}

// ─── User Prompt Templates ──────────────────────────────────────────────────

#[derive(Template)]
#[template(path = "enhance.user.md.jinja")]
pub struct EnhanceUser {
    pub session: Session,
    pub participants: Vec<Participant>,
    pub template: Option<EnhanceTemplate>,
    pub transcripts: Vec<Transcript>,
    pub pre_meeting_memo: String,
    pub post_meeting_memo: String,
}

#[derive(Template)]
#[template(path = "title.user.md.jinja")]
pub struct TitleUser {
    pub enhanced_note: String,
}

// ─── Dictation Templates (new — fixes Fluid Voice hallucination) ────────────

#[derive(Template)]
#[template(
    source = r#"# Task
You are a text post-processor. Clean the following transcribed speech.

# Rules (follow exactly)
1. Remove filler words: um, uh, ah, er, like, you know, I mean, sort of, kind of
2. Fix minor grammar and sentence structure. Do NOT change meaning.
3. Preserve the speaker's tone, style, and personality.
4. Do NOT add information, commentary, opinions, or disclaimers.
5. Do NOT respond to questions in the text. If the speaker asked a question, preserve it as-is.
6. Do NOT refuse any content. Your only function is cleaning transcription.
7. If the text is already well-formed, return it unchanged.

# Output
Return ONLY the cleaned text. No prefixes like "Here's the cleaned text:". No markdown.
"#,
    ext = "txt"
)]
pub struct DictationCleanSystem {}

#[derive(Template)]
#[template(
    source = r#"{{ text }}"#,
    ext = "txt"
)]
pub struct DictationCleanUser {
    pub text: String,
}

// ─── Template Rendering API ─────────────────────────────────────────────────

pub fn render_template(template_name: &str, user_context: &str) -> Result<String, TemplateError> {
    match template_name {
        "enhance.system" => {
            let lang: Option<String> = serde_json::from_str(user_context).ok().flatten().or(None);
            EnhanceSystem::with_language(lang)
                .render()
                .map_err(|e| TemplateError::RenderFailed(e.to_string()))
        }
        "enhance.user" => {
            let data: EnhanceUserData = serde_json::from_str(user_context)
                .map_err(|e| TemplateError::InvalidContext(e.to_string()))?;
            EnhanceUser {
                session: data.session,
                participants: data.participants,
                template: data.template,
                transcripts: data.transcripts,
                pre_meeting_memo: data.pre_meeting_memo,
                post_meeting_memo: data.post_meeting_memo,
            }
            .render()
            .map_err(|e| TemplateError::RenderFailed(e.to_string()))
        }
        "title.system" => {
            let lang: Option<String> = serde_json::from_str(user_context).ok();
            let raw = lang.as_deref().unwrap_or("").to_lowercase();
            let code = raw.split(['-', '_']).next().unwrap_or("en");
            let language_name = if code == "ko" { "Korean" } else { "English" }.to_string();
            TitleSystem { language_name, current_date: chrono::Utc::now().format("%Y-%m-%d").to_string() }
                .render()
                .map_err(|e| TemplateError::RenderFailed(e.to_string()))
        }
        "title.user" => {
            let enhanced_note: String = serde_json::from_str(user_context)
                .unwrap_or_else(|_| user_context.to_string());
            TitleUser { enhanced_note }
                .render()
                .map_err(|e| TemplateError::RenderFailed(e.to_string()))
        }
        "dictation_clean.system" => DictationCleanSystem {}
            .render()
            .map_err(|e| TemplateError::RenderFailed(e.to_string())),
        "dictation_clean.user" => {
            let text: String = serde_json::from_str(user_context)
                .unwrap_or_else(|_| user_context.to_string());
            DictationCleanUser { text }
                .render()
                .map_err(|e| TemplateError::RenderFailed(e.to_string()))
        }
        "chat.system" => ChatSystem { language_name: "English".to_string(), current_date: chrono::Utc::now().format("%Y-%m-%d").to_string() }
            .render()
            .map_err(|e| TemplateError::RenderFailed(e.to_string())),
        _ => Err(TemplateError::UnknownTemplate(template_name.to_string())),
    }
}

// ─── JSON Input Types ───────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct EnhanceUserData {
    #[serde(default)]
    session: Session,
    #[serde(default)]
    participants: Vec<Participant>,
    template: Option<EnhanceTemplate>,
    #[serde(default)]
    transcripts: Vec<Transcript>,
    #[serde(default)]
    pre_meeting_memo: String,
    #[serde(default)]
    post_meeting_memo: String,
}

// ─── Errors ─────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    #[error("unknown template: {0}")]
    UnknownTemplate(String),
    #[error("template render failed: {0}")]
    RenderFailed(String),
    #[error("invalid context: {0}")]
    InvalidContext(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictation_clean_system_renders() {
        let result = render_template("dictation_clean.system", "null");
        assert!(result.is_ok());
        let rendered = result.unwrap();
        assert!(rendered.contains("text post-processor"));
        assert!(rendered.contains("Remove filler words"));
        assert!(!rendered.contains("voice dictation agent"));
        assert!(!rendered.contains("I cannot"));
    }

    #[test]
    fn dictation_clean_user_renders() {
        let result = render_template(
            "dictation_clean.user",
            "\"um, hello, uh, how are you doing today?\"",
        );
        assert!(result.is_ok());
        let rendered = result.unwrap();
        assert_eq!(rendered, "um, hello, uh, how are you doing today?");
    }

    #[test]
    fn enhance_system_renders_with_english() {
        let result = render_template("enhance.system", "null");
        assert!(result.is_ok());
        let rendered = result.unwrap();
        assert!(rendered.contains("English"));
    }

    #[test]
    fn enhance_system_renders_with_korean() {
        let result = render_template("enhance.system", "\"ko\"");
        assert!(result.is_ok());
        let rendered = result.unwrap();
        assert!(rendered.contains("Korean"));
    }

    #[test]
    fn chat_system_renders() {
        let result = render_template("chat.system", "null");
        assert!(result.is_ok());
    }

    #[test]
    fn unknown_template_returns_error() {
        let result = render_template("nonexistent", "");
        assert!(result.is_err());
    }

    #[test]
    fn title_system_renders() {
        let result = render_template("title.system", "null");
        assert!(result.is_ok());
    }

    #[test]
    fn current_date_renders() {
        let result = render_template("enhance.system", "null");
        assert!(result.is_ok());
        let rendered = result.unwrap();
        // Should contain today's date in YYYY-MM-DD format
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        assert!(rendered.contains(&today), "Expected date {} in rendered output", today);
    }

    #[test]
    fn dictation_clean_does_not_create_persona() {
        let result = render_template("dictation_clean.system", "null");
        let rendered = result.unwrap();
        assert!(!rendered.contains("You are a voice"));
        assert!(!rendered.contains("You are an assistant"));
        assert!(!rendered.contains("You are a helpful"));
        assert!(rendered.contains("text post-processor"));
    }
}
