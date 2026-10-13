use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Language {
    #[serde(rename = "en")]
    #[default]
    En,
    #[serde(rename = "it")]
    It,
    #[serde(rename = "es")]
    Es,
    #[serde(rename = "fr")]
    Fr,
    #[serde(rename = "de")]
    De,
    #[serde(rename = "zh")]
    Zh,
    #[serde(rename = "ja")]
    Ja,
}

impl Language {
    pub const ALL: [Language; 7] = [
        Language::En,
        Language::It,
        Language::Es,
        Language::Fr,
        Language::De,
        Language::Zh,
        Language::Ja,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Language::En => "en",
            Language::It => "it",
            Language::Es => "es",
            Language::Fr => "fr",
            Language::De => "de",
            Language::Zh => "zh",
            Language::Ja => "ja",
        }
    }

    pub fn native_name(self) -> &'static str {
        match self {
            Language::En => "English",
            Language::It => "Italiano",
            Language::Es => "Español",
            Language::Fr => "Français",
            Language::De => "Deutsch",
            Language::Zh => "中文",
            Language::Ja => "日本語",
        }
    }

    pub fn from_code(s: &str) -> Option<Language> {
        match s {
            "en" => Some(Language::En),
            "it" => Some(Language::It),
            "es" => Some(Language::Es),
            "fr" => Some(Language::Fr),
            "de" => Some(Language::De),
            "zh" => Some(Language::Zh),
            "ja" => Some(Language::Ja),
            _ => None,
        }
    }

    pub fn is_cjk(self) -> bool {
        matches!(self, Language::Zh | Language::Ja)
    }
}

pub fn tr(language: Language, english: &str) -> &str {
    match language {
        Language::En => english,
        Language::It => it(english),
        Language::Es => es(english),
        Language::Fr => fr(english),
        Language::De => de(english),
        Language::Zh => zh(english),
        Language::Ja => ja(english),
    }
}

fn it(english: &str) -> &str {
    match english {
        "Check" => "Controllo",
        "Design checks" => "Controlli di progettazione",
        "Empty frame" => "Frame vuoto",
        "Give feedback, ask a question, or just leave a note. Click anywhere on the canvas to leave a comment." => "Lascia un feedback, fai una domanda o semplicemente lascia una nota. Fai clic in un punto qualsiasi della tela per lasciare un commento.",
        "Hidden layer" => "Livello nascosto",
        "No components match." => "Nessun componente corrispondente.",
        "No components yet. Select layers and press ⌥⌘K." => "Ancora nessun componente. Seleziona dei livelli e premi ⌥⌘K.",
        "No saved versions. Save one to come back to it later." => "Nessuna versione salvata. Salva una versione per poter tornare in seguito.",
        "Off the pixel grid" => "Fuori dalla griglia di pixel",
        "Paint style not used" => "Stile riempimento non utilizzato",
        "Raw value" => "Valore grezzo",
        "Reopen" => "Riapri",
        "Resolve" => "Risolvi",
        "Same as now." => "Uguale a ora.",
        "Text style not used" => "Stile testo non utilizzato",
        "Version history" => "Cronologia versioni",
        _ => english,
    }
}

fn es(english: &str) -> &str {
    match english {
        "Check" => "Comprobación",
        "Design checks" => "Comprobaciones de diseño",
        "Empty frame" => "Marco vacío",
        "Give feedback, ask a question, or just leave a note. Click anywhere on the canvas to leave a comment." => "Da tu opinión, haz una pregunta o simplemente deja una nota. Haz clic en cualquier parte del lienzo para dejar un comentario.",
        "Hidden layer" => "Capa oculta",
        "No components match." => "Ningún componente coincide.",
        "No components yet. Select layers and press ⌥⌘K." => "Aún no hay componentes. Selecciona capas y pulsa ⌥⌘K.",
        "No saved versions. Save one to come back to it later." => "No hay versiones guardadas. Guarda una para volver a ella más tarde.",
        "Off the pixel grid" => "Fuera de la cuadrícula de píxeles",
        "Paint style not used" => "Estilo de relleno no utilizado",
        "Raw value" => "Valor sin procesar",
        "Reopen" => "Reabrir",
        "Resolve" => "Resolver",
        "Same as now." => "Igual que ahora.",
        "Text style not used" => "Estilo de texto no utilizado",
        "Version history" => "Historial de versiones",
        _ => english,
    }
}

fn fr(english: &str) -> &str {
    match english {
        "Check" => "Vérification",
        "Design checks" => "Vérifications de conception",
        "Empty frame" => "Cadre vide",
        "Give feedback, ask a question, or just leave a note. Click anywhere on the canvas to leave a comment." => "Donnez votre avis, posez une question ou laissez simplement une note. Cliquez n'importe où sur le canevas pour laisser un commentaire.",
        "Hidden layer" => "Calque masqué",
        "No components match." => "Aucun composant correspondant.",
        "No components yet. Select layers and press ⌥⌘K." => "Aucun composant pour le moment. Sélectionnez des calques et appuyez sur ⌥⌘K.",
        "No saved versions. Save one to come back to it later." => "Aucune version enregistrée. Enregistrez-en une pour y revenir plus tard.",
        "Off the pixel grid" => "Hors de la grille de pixels",
        "Paint style not used" => "Style de remplissage non utilisé",
        "Raw value" => "Valeur brute",
        "Reopen" => "Rouvrir",
        "Resolve" => "Résoudre",
        "Same as now." => "Identique à maintenant.",
        "Text style not used" => "Style de texte non utilisé",
        "Version history" => "Historique des versions",
        _ => english,
    }
}

fn de(english: &str) -> &str {
    match english {
        "Check" => "Prüfung",
        "Design checks" => "Design-Prüfungen",
        "Empty frame" => "Leerer Frame",
        "Give feedback, ask a question, or just leave a note. Click anywhere on the canvas to leave a comment." => "Gib Feedback, stelle eine Frage oder hinterlasse einfach eine Notiz. Klicke irgendwo auf dem Canvas, um einen Kommentar zu hinterlassen.",
        "Hidden layer" => "Ausgeblendete Ebene",
        "No components match." => "Keine übereinstimmenden Komponenten.",
        "No components yet. Select layers and press ⌥⌘K." => "Noch keine Komponenten. Ebenen auswählen und ⌥⌘K drücken.",
        "No saved versions. Save one to come back to it later." => "Keine gespeicherten Versionen. Speichere eine, um später darauf zurückzugreifen.",
        "Off the pixel grid" => "Außerhalb des Pixelrasters",
        "Paint style not used" => "Füllstil nicht verwendet",
        "Raw value" => "Rohwert",
        "Reopen" => "Wieder öffnen",
        "Resolve" => "Auflösen",
        "Same as now." => "Wie aktuell.",
        "Text style not used" => "Textstil nicht verwendet",
        "Version history" => "Versionsverlauf",
        _ => english,
    }
}

fn zh(english: &str) -> &str {
    match english {
        "Check" => "检查",
        "Design checks" => "设计检查",
        "Empty frame" => "空框架",
        "Give feedback, ask a question, or just leave a note. Click anywhere on the canvas to leave a comment." => "提供反馈、提出问题或留下备注。点击画布上的任意位置以发表评论。",
        "Hidden layer" => "隐藏图层",
        "No components match." => "没有匹配的组件。",
        "No components yet. Select layers and press ⌥⌘K." => "还没有组件。选择图层并按 ⌥⌘K。",
        "No saved versions. Save one to come back to it later." => "没有保存的版本。保存一个版本，以便以后返回查看。",
        "Off the pixel grid" => "偏离像素网格",
        "Paint style not used" => "填充样式未使用",
        "Raw value" => "原始值",
        "Reopen" => "重新打开",
        "Resolve" => "解决",
        "Same as now." => "与当前相同。",
        "Text style not used" => "文本样式未使用",
        "Version history" => "版本历史",
        _ => english,
    }
}

fn ja(english: &str) -> &str {
    match english {
        "Check" => "チェック",
        "Design checks" => "デザインチェック",
        "Empty frame" => "空のフレーム",
        "Give feedback, ask a question, or just leave a note. Click anywhere on the canvas to leave a comment." => "フィードバックを送ったり、質問したり、メモを残したりできます。キャンバス上の任意の場所をクリックしてコメントを残してください。",
        "Hidden layer" => "非表示レイヤー",
        "No components match." => "一致するコンポーネントがありません。",
        "No components yet. Select layers and press ⌥⌘K." => "コンポーネントがまだありません。レイヤーを選択して ⌥⌘K を押してください。",
        "No saved versions. Save one to come back to it later." => "保存されたバージョンがありません。後で戻れるように保存しましょう。",
        "Off the pixel grid" => "ピクセルグリッドからずれています",
        "Paint style not used" => "塗りスタイルは未使用です",
        "Raw value" => "生の値",
        "Reopen" => "再開",
        "Resolve" => "解決",
        "Same as now." => "現在と同じです。",
        "Text style not used" => "テキストスタイルは未使用です",
        "Version history" => "バージョン履歴",
        _ => english,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_is_the_fallback() {
        assert_eq!(tr(Language::It, "No such string"), "No such string");
        assert_eq!(tr(Language::Zh, "No such string"), "No such string");
        assert_eq!(tr(Language::En, "Anything"), "Anything");
    }

    #[test]
    fn language_round_trip() {
        for l in Language::ALL {
            assert_eq!(Language::from_code(l.as_str()), Some(l));
        }
        assert_eq!(Language::from_code("xx"), None);
    }
}
