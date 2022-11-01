use std::sync::Arc;
use tokio::task;

pub struct WebhookPublisher {
    endpoint: Arc<http::Uri>,
    api_key: Arc<String>,
    template: String,
}

impl WebhookPublisher {
    pub fn new(endpoint: http::Uri, api_key: String, template: String) -> Self {
        WebhookPublisher {
            endpoint: Arc::new(endpoint),
            api_key: Arc::new(api_key),
            template,
        }
    }

    pub async fn publish(&self, matched_groups: Vec<Vec<String>>) {
        let tasks = matched_groups
            .iter()
            .map(|g| self.publish_group(&g))
            .collect::<Vec<task::JoinHandle<()>>>();

        let result = futures::future::join_all(tasks).await;
        println!("{:?}", result);
    }

    pub fn publish_group(&self, group: &[String]) -> task::JoinHandle<()> {
        tracing::debug!(group = ?group, "building POST body");
        // Replace the items in the template with matches from the group.
        let body = group
            .iter()
            .enumerate()
            .fold(self.template.clone(), |body, (idx, repl)| {
                let idx_thing = format!("${{{}}}", idx);
                body.replace(&idx_thing, repl)
            });
        tracing::debug!(body = body);

        let join = task::spawn({
            let endpoint = self.endpoint.clone();
            let api_key = self.api_key.clone();

            async move {
                let client = reqwest::Client::new();
                let res = client
                    .post(endpoint.to_string())
                    .header("X-Api-Key", api_key.to_string())
                    .header("Content-Type", "application/json")
                    .body(body)
                    .send()
                    .await;

                if let Err(e) = res {
                    tracing::error!("webhook POST error: {}", e);
                }
            }
        });

        return join;
    }
}
