use irc::client::prelude::*;
use regex::Regex;

use crate::webhook_publisher;

pub struct MessageHandler {
    message_publisher: webhook_publisher::WebhookPublisher,
    re: Regex,
}

impl MessageHandler {
    pub fn new(
        search_pattern: &str,
        message_publisher: webhook_publisher::WebhookPublisher,
    ) -> Self {
        MessageHandler {
            message_publisher,
            re: Regex::new(search_pattern).unwrap(),
        }
    }

    pub async fn handle_msg(&mut self, msg: Message) {
        if let Some(content) = get_content(&msg.to_string()) {
            tracing::debug!(msg = content, "checking for matches");
            if !self.re.is_match(&content) {
                return;
            }
            tracing::info!(content, "matched");

            let groups = match_groups(&self.re, &content);
            self.message_publisher.publish(groups).await;
        }
    }
}

fn get_content(m: &str) -> Option<String> {
    // Skip the first character, which is always a :, then find the next :
    let mut chrs = m.chars().skip(1);

    if let Some(_) = chrs.find(|&c| c == ':') {
        // Make sure the trailing newline is removed.
        Some(chrs.collect::<String>().trim().to_string())
    } else {
        None
    }
}

fn match_groups(re: &regex::Regex, content: &str) -> Vec<Vec<String>> {
    re.captures_iter(content)
        .map(|group| {
            group
                .iter()
                .filter_map(|mat| Some(mat?.as_str().to_string()))
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_content() {
        let input = ":first part:Hello this is a message".to_string();
        assert_eq!(
            get_content(&input),
            Some("Hello this is a message".to_string())
        );
    }

    #[test]
    fn test_match_groups() {
        let content = r#"Main message 1capture match2 text 1another match2"#;

        let search_pattern = r#"\d(.+?)\d"#;
        let re = Regex::new(&search_pattern).unwrap();

        let got = match_groups(&re, content);

        assert_eq!(
            got,
            vec![
                vec!["1capture match2".to_string(), "capture match".to_string()],
                vec!["1another match2".to_string(), "another match".to_string()]
            ]
        )
    }
}
