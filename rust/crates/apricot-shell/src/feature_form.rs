//! Shared, typed form contract for native feature dialogs.
#![allow(clippy::pedantic, clippy::style)] // 2.1 draft: fully linted when its feature unit is finished.

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct FeatureForm {
    pub title: String,
    pub fields: Vec<FormField>,
    pub submit: String,
    pub cancel: String,
    /// "{field}" is replaced by the label of the rejected field.
    pub invalid_value: String,
    /// "{field}", "{minimum}" and "{maximum}" describe a rejected number.
    pub number_range: String,
    /// Index of the field that has the focus when the form opens.
    pub initial_focus: usize,
}

pub type FormValues = BTreeMap<String, FormValue>;

#[derive(Clone, Debug, PartialEq)]
pub struct FormField {
    pub id: String,
    pub label: String,
    pub kind: FormFieldKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FormFieldKind {
    Text {
        value: String,
        secret: bool,
    },
    Number {
        value: f64,
        minimum: f64,
        maximum: f64,
    },
    Choice {
        options: Vec<String>,
        selected: usize,
    },
    Checkbox {
        checked: bool,
    },
    ReadOnly {
        value: String,
    },
    /// A path typed or chosen with a Browse button; `extensions` filter the
    /// file dialog.
    File {
        value: String,
        extensions: Vec<String>,
        browse: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum FormValue {
    Text(String),
    Number(f64),
    Choice(usize),
    Checkbox(bool),
}

impl FormValue {
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text(value) => Some(value),
            _ => None,
        }
    }

    #[must_use]
    pub const fn number(&self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub const fn choice(&self) -> Option<usize> {
        match self {
            Self::Choice(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub const fn checked(&self) -> Option<bool> {
        match self {
            Self::Checkbox(value) => Some(*value),
            _ => None,
        }
    }
}

impl FeatureForm {
    #[must_use]
    pub fn new(title: impl Into<String>, fields: Vec<FormField>) -> Self {
        Self {
            title: title.into(),
            fields,
            submit: "Save".to_owned(),
            cancel: "Cancel".to_owned(),
            invalid_value: "{field}: the value is not valid.".to_owned(),
            number_range: "{field}: enter a number from {minimum} to {maximum}.".to_owned(),
            initial_focus: 0,
        }
    }

    /// A form whose buttons and validation messages use the interface language.
    #[must_use]
    pub fn localized(
        catalog: &apricot_core::TranslationCatalog,
        title: impl Into<String>,
        fields: Vec<FormField>,
    ) -> Self {
        Self {
            title: title.into(),
            fields,
            submit: catalog.text("save").to_owned(),
            cancel: catalog.text("cancel").to_owned(),
            invalid_value: catalog.text("form_invalid_value").to_owned(),
            number_range: catalog.text("form_number_range").to_owned(),
            initial_focus: 0,
        }
    }

    /// The sentence shown and spoken when `validate` rejects `label`.
    #[must_use]
    pub fn error_message(&self, label: &str) -> String {
        let field = self.fields.iter().find(|field| field.label == label);
        if let Some(FormFieldKind::Number {
            minimum, maximum, ..
        }) = field.map(|field| &field.kind)
        {
            return self
                .number_range
                .replace("{field}", label)
                .replace("{minimum}", &format_number(*minimum))
                .replace("{maximum}", &format_number(*maximum));
        }
        self.invalid_value.replace("{field}", label)
    }

    /// The position of the field whose label `validate` returned.
    #[must_use]
    pub fn field_index(&self, label: &str) -> Option<usize> {
        self.fields.iter().position(|field| field.label == label)
    }

    /// Validates the complete response before any settings or side effects are committed.
    /// Numeric edit fields may supply text; both native adapters share this parser.
    ///
    /// # Errors
    /// Returns the first invalid field's label. The form remains open for correction.
    pub fn validate(&self, values: &FormValues) -> Result<FormValues, String> {
        let mut result = BTreeMap::new();
        for field in &self.fields {
            if result.contains_key(&field.id) {
                return Err(field.label.clone());
            }
            let value = match &field.kind {
                FormFieldKind::ReadOnly { .. } => continue,
                FormFieldKind::Text { .. } | FormFieldKind::File { .. } => {
                    match values.get(&field.id) {
                        Some(FormValue::Text(value)) => FormValue::Text(value.clone()),
                        _ => return Err(field.label.clone()),
                    }
                }
                FormFieldKind::Number {
                    minimum, maximum, ..
                } => {
                    let value = match values.get(&field.id) {
                        Some(FormValue::Number(value)) => Some(*value),
                        Some(FormValue::Text(value)) => {
                            value.trim().replace(',', ".").parse::<f64>().ok()
                        }
                        _ => None,
                    }
                    .filter(|value| value.is_finite() && value >= minimum && value <= maximum);
                    FormValue::Number(value.ok_or_else(|| field.label.clone())?)
                }
                FormFieldKind::Choice { options, .. } => match values.get(&field.id) {
                    Some(FormValue::Choice(index)) if *index < options.len() => {
                        FormValue::Choice(*index)
                    }
                    _ => return Err(field.label.clone()),
                },
                FormFieldKind::Checkbox { .. } => match values.get(&field.id) {
                    Some(FormValue::Checkbox(value)) => FormValue::Checkbox(*value),
                    _ => return Err(field.label.clone()),
                },
            };
            result.insert(field.id.clone(), value);
        }
        Ok(result)
    }
}

impl FormField {
    #[must_use]
    pub fn text(id: &str, label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            kind: FormFieldKind::Text {
                value: value.into(),
                secret: false,
            },
        }
    }

    #[must_use]
    pub fn secret(id: &str, label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            kind: FormFieldKind::Text {
                value: value.into(),
                secret: true,
            },
        }
    }

    #[must_use]
    pub fn number(
        id: &str,
        label: impl Into<String>,
        value: f64,
        minimum: f64,
        maximum: f64,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            kind: FormFieldKind::Number {
                value,
                minimum,
                maximum,
            },
        }
    }

    #[must_use]
    pub fn choice(
        id: &str,
        label: impl Into<String>,
        options: Vec<String>,
        selected: usize,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            kind: FormFieldKind::Choice { options, selected },
        }
    }

    #[must_use]
    pub fn checkbox(id: &str, label: impl Into<String>, checked: bool) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            kind: FormFieldKind::Checkbox { checked },
        }
    }

    #[must_use]
    pub fn file(
        id: &str,
        label: impl Into<String>,
        value: impl Into<String>,
        extensions: &[&str],
        browse: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            kind: FormFieldKind::File {
                value: value.into(),
                extensions: extensions.iter().map(|value| (*value).to_owned()).collect(),
                browse: browse.into(),
            },
        }
    }

    #[must_use]
    pub fn read_only(id: &str, label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            kind: FormFieldKind::ReadOnly {
                value: value.into(),
            },
        }
    }
}

/// Whole numbers without a decimal point, others as typed (2.5, 0.1).
#[must_use]
pub fn format_number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{value:.0}")
    } else {
        let text = format!("{value:.3}");
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_fields_get_a_sentence_with_the_allowed_range() {
        let form = FeatureForm::new(
            "Timer",
            vec![
                FormField::number("minutes", "Minutes", 30.0, 1.0, 1440.0),
                FormField::text("name", "Name", ""),
            ],
        );
        assert_eq!(
            form.error_message("Minutes"),
            "Minutes: enter a number from 1 to 1440."
        );
        assert_eq!(form.error_message("Name"), "Name: the value is not valid.");
        assert_eq!(form.field_index("Name"), Some(1));
        assert_eq!(format_number(0.016_666_7), "0.017");
        assert_eq!(format_number(2.5), "2.5");
    }

    #[test]
    fn rejects_incomplete_invalid_and_nonfinite_input_before_commit() {
        let form = FeatureForm::new(
            "Timer",
            vec![FormField::number("minutes", "Minutes", 30.0, 1.0, 1440.0)],
        );
        for raw in ["NaN", "inf", "-1", "1441", "oops"] {
            assert_eq!(
                form.validate(&BTreeMap::from([(
                    "minutes".into(),
                    FormValue::Text(raw.into())
                )])),
                Err("Minutes".into())
            );
        }
        assert!(form.validate(&BTreeMap::new()).is_err());
        let result = form
            .validate(&BTreeMap::from([(
                "minutes".into(),
                FormValue::Text("2.5".into()),
            )]))
            .unwrap();
        assert_eq!(result["minutes"], FormValue::Number(2.5));
    }

    #[test]
    fn preserves_secret_text_without_normalizing_and_checks_choice_bounds() {
        let form = FeatureForm::new(
            "Source",
            vec![
                FormField::secret("password", "Password", ""),
                FormField::choice("type", "Type", vec!["A".into()], 0),
            ],
        );
        let mut raw = BTreeMap::from([
            ("password".into(), FormValue::Text("  secret  ".into())),
            ("type".into(), FormValue::Choice(0)),
        ]);
        assert_eq!(
            form.validate(&raw).unwrap()["password"].text(),
            Some("  secret  ")
        );
        raw.insert("type".into(), FormValue::Choice(1));
        assert_eq!(form.validate(&raw), Err("Type".into()));
    }
}
