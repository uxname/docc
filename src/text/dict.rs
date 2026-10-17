// Built-in dictionaries (Russian and English); docc.json extends them.

/// Forbidden: DOC301.
pub const FORBIDDEN: &[&str] = &[
    "быстро", "удобно", "интуитивно", "и т.д.", "и т.п.",
    "fast", "easy", "intuitive", "user-friendly", "etc.",
];

/// Hedging: DOC352.
pub const HEDGING: &[&str] = &[
    "возможно", "вероятно", "как правило", "по-видимому", "в некоторых случаях", "наверное", "скорее всего",
    "несколько", "нескольких", "нескольким", "несколькими",
    "некоторые", "некоторых", "некоторым", "некоторыми", "различные", "различных", "различным", "различными",
    "maybe", "probably", "usually", "might", "perhaps", "several", "various",
];

/// Timeless: DOC353.
pub const TIMELESS: &[&str] = &[
    "сейчас", "недавно", "пока", "в настоящее время", "скоро", "в будущем", "новый", "новая", "новое", "новые",
    "currently", "recently", "soon", "new", "at the moment",
];

/// Placeholders: DOC303.
/// A value that is only a dash ("—", "-", "–") is one too; see is_dash_only.
pub const PLACEHOLDERS: &[&str] = &[
    "TODO", "TBD", "FIXME", "lorem", "???", "N/A", "см. выше", "работает корректно", "по необходимости", "see above",
    "works correctly",
];

/// IsDashOnly reports whether a value is only a dash: a placeholder for
/// "nothing to say" (DOC303).
pub fn is_dash_only(s: &str) -> bool {
    matches!(s.trim(), "—" | "-" | "–")
}
