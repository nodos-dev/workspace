#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LangTool {
    CppCMake,
}

impl LangTool {
    pub const POSSIBLE_VALUES: [&'static str; 1] = ["cpp/cmake"];

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "cpp/cmake" => Some(LangTool::CppCMake),
            _ => None,
        }
    }

    pub fn lang(&self) -> &'static str {
        match self {
            LangTool::CppCMake => "cpp",
        }
    }

    pub fn tool(&self) -> &'static str {
        match self {
            LangTool::CppCMake => "cmake",
        }
    }
}

impl std::fmt::Display for LangTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LangTool::CppCMake => write!(f, "cpp/cmake"),
        }
    }
}
