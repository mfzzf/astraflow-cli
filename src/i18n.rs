use crate::cli::Language;

pub const BANNER: &str = r#"
 █████╗ ███████╗████████╗██████╗  █████╗ ███████╗██╗      ██████╗ ██╗    ██╗
██╔══██╗██╔════╝╚══██╔══╝██╔══██╗██╔══██╗██╔════╝██║     ██╔═══██╗██║    ██║
███████║███████╗   ██║   ██████╔╝███████║█████╗  ██║     ██║   ██║██║ █╗ ██║
██╔══██║╚════██║   ██║   ██╔══██╗██╔══██║██╔══╝  ██║     ██║   ██║██║███╗██║
██║  ██║███████║   ██║   ██║  ██║██║  ██║██║     ███████╗╚██████╔╝╚███╔███╔╝
╚═╝  ╚═╝╚══════╝   ╚═╝   ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝     ╚══════╝ ╚═════╝  ╚══╝╚══╝
"#;

#[derive(Debug, Clone, Copy)]
pub struct Messages {
    pub language: Language,
}

impl Messages {
    pub fn new(language: Language) -> Self {
        Self { language }
    }

    pub fn choose_language(self) -> &'static str {
        match self.language {
            Language::En => "Choose a language",
            Language::Zh => "选择语言",
        }
    }

    pub fn opening_browser(self) -> &'static str {
        match self.language {
            Language::En => "Opening UCloud sign-in in your browser…",
            Language::Zh => "正在浏览器中打开 UCloud 登录…",
        }
    }

    pub fn browser_url(self) -> &'static str {
        match self.language {
            Language::En => "If the browser did not open, visit:",
            Language::Zh => "如果浏览器未打开，请访问：",
        }
    }

    pub fn paste_callback(self) -> &'static str {
        match self.language {
            Language::En => {
                "For SSH, paste the final localhost callback URL here (or wait for this device)"
            }
            Language::Zh => "SSH 环境可在此粘贴最终的 localhost 回调 URL（或等待当前设备回调）",
        }
    }

    pub fn choose_key(self) -> &'static str {
        match self.language {
            Language::En => "Choose a ModelVerse API key",
            Language::Zh => "选择 ModelVerse API Key",
        }
    }

    pub fn ready(self) -> &'static str {
        match self.language {
            Language::En => "AstraFlow is ready.",
            Language::Zh => "AstraFlow 已就绪。",
        }
    }
}
