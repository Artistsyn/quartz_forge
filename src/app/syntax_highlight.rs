use std::sync::Arc;

use eframe::egui;

pub fn highlight_rust_layout(text: &str, wrap_width: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = wrap_width;

    let font = egui::FontId::monospace(13.0);
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let c = chars[i];

        if c == '/' && i + 1 < len && chars[i + 1] == '/' {
            let start = i;
            while i < len && chars[i] != '\n' {
                i += 1;
            }
            let span: String = chars[start..i].iter().collect();
            append_colored(&mut job, &span, &font, egui::Color32::from_rgb(106, 115, 125));
            continue;
        }

        if c == '"' {
            let start = i;
            i += 1;
            while i < len {
                if chars[i] == '\\' && i + 1 < len {
                    i += 2;
                } else if chars[i] == '"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            let span: String = chars[start..i].iter().collect();
            append_colored(&mut job, &span, &font, egui::Color32::from_rgb(152, 195, 121));
            continue;
        }

        if c.is_ascii_digit() {
            let start = i;
            while i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_') {
                i += 1;
            }
            let span: String = chars[start..i].iter().collect();
            append_colored(&mut job, &span, &font, egui::Color32::from_rgb(209, 154, 102));
            continue;
        }

        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let is_fn_call = i < len && chars[i] == '(';
            let color = if is_keyword(&word) {
                egui::Color32::from_rgb(198, 120, 221)
            } else if is_type_name(&word) {
                egui::Color32::from_rgb(86, 182, 194)
            } else if is_fn_call {
                egui::Color32::from_rgb(97, 175, 239)
            } else {
                egui::Color32::from_rgb(190, 190, 190)
            };
            append_colored(&mut job, &word, &font, color);
            continue;
        }

        append_colored(&mut job, &c.to_string(), &font, egui::Color32::from_rgb(170, 170, 170));
        i += 1;
    }

    job
}

pub fn code_layouter(ui: &egui::Ui, text: &str, wrap_width: f32) -> Arc<egui::text::Galley> {
    let job = highlight_rust_layout(text, wrap_width);
    ui.fonts(|f| f.layout_job(job))
}

fn append_colored(job: &mut egui::text::LayoutJob, text: &str, font: &egui::FontId, color: egui::Color32) {
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: font.clone(),
            color,
            ..Default::default()
        },
    );
}

fn is_keyword(word: &str) -> bool {
    matches!(
        word,
        "as"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
    )
}

fn is_type_name(word: &str) -> bool {
    matches!(
        word,
        "i8"
            | "i16"
            | "i32"
            | "i64"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "usize"
            | "f32"
            | "f64"
            | "bool"
            | "char"
            | "str"
            | "String"
            | "Vec"
            | "Option"
            | "Result"
            | "Canvas"
            | "GameObject"
            | "Action"
            | "Target"
            | "GameEvent"
            | "Location"
    )
}
