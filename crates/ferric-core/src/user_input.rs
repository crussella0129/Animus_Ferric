use serde::{Deserialize, Serialize};

/// A structured clarification that the agent needs before it can safely
/// continue the original objective.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserInputRequest {
    /// The concrete decision or missing fact the user must supply.
    pub question: String,
    /// Why the answer is material and cannot be recovered from the workspace.
    pub context: String,
    /// Suggested answers. An empty list means the response is free-form.
    pub options: Vec<String>,
}

impl UserInputRequest {
    /// Validate semantic constraints that JSON types alone cannot express.
    pub fn validate(&self) -> Result<(), UserInputValidationError> {
        if self.question.trim().is_empty() {
            return Err(UserInputValidationError::EmptyQuestion);
        }
        if self.context.trim().is_empty() {
            return Err(UserInputValidationError::EmptyContext);
        }
        if let Some(index) = self
            .options
            .iter()
            .position(|option| option.trim().is_empty())
        {
            return Err(UserInputValidationError::EmptyOption { index });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UserInputValidationError {
    #[error("question must not be empty")]
    EmptyQuestion,
    #[error("context must not be empty")]
    EmptyContext,
    #[error("option at index {index} must not be empty")]
    EmptyOption { index: usize },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_request() -> UserInputRequest {
        UserInputRequest {
            question: "Which database should this target?".to_string(),
            context: "The repository contains two adapters and documents no default.".to_string(),
            options: vec!["SQLite".to_string(), "PostgreSQL".to_string()],
        }
    }

    #[test]
    fn valid_request_roundtrips_and_allows_free_form() {
        let request = valid_request();
        request.validate().unwrap();

        let encoded = serde_json::to_value(&request).unwrap();
        assert_eq!(
            serde_json::from_value::<UserInputRequest>(encoded).unwrap(),
            request
        );

        let mut free_form = valid_request();
        free_form.options.clear();
        free_form.validate().unwrap();
    }

    #[test]
    fn validation_rejects_blank_required_text() {
        let mut request = valid_request();
        request.question = " \t".to_string();
        assert_eq!(
            request.validate(),
            Err(UserInputValidationError::EmptyQuestion)
        );

        let mut request = valid_request();
        request.context = "\n".to_string();
        assert_eq!(
            request.validate(),
            Err(UserInputValidationError::EmptyContext)
        );

        let mut request = valid_request();
        request.options[1] = "  ".to_string();
        assert_eq!(
            request.validate(),
            Err(UserInputValidationError::EmptyOption { index: 1 })
        );
    }

    #[test]
    fn serde_requires_every_field_and_rejects_unknown_fields() {
        for malformed in [
            json!({"context": "why", "options": []}),
            json!({"question": "what?", "options": []}),
            json!({"question": "what?", "context": "why"}),
            json!({
                "question": "what?",
                "context": "why",
                "options": [],
                "unexpected": true
            }),
            json!({"question": 7, "context": "why", "options": []}),
            json!({"question": "what?", "context": "why", "options": [7]}),
        ] {
            assert!(
                serde_json::from_value::<UserInputRequest>(malformed).is_err(),
                "malformed request must be rejected"
            );
        }
    }
}
