use futures::stream;
use std::sync::Arc;
use tokio::task;

pub struct WebhookPublisher {
    endpoint: http::Uri,
    api_key: String,
    template: String,
}

impl WebhookPublisher {
    pub fn new(endpoint: http::Uri, api_key: String, template: String) -> Self {
        WebhookPublisher {
            endpoint,
            api_key,
            template,
        }
    }

    pub async fn publish(&self, params: Vec<Vec<String>>) {
        let ep = Arc::new(self.endpoint.to_string());
        let key = Arc::new(self.api_key.clone());
        let template = Arc::new(self.template.clone());

        let tasks = params
            .into_iter()
            .map(|p_set| {
                task::spawn({
                    let ep_clone = Arc::clone(&ep);
                    let key_clone = Arc::clone(&key);
                    let template_clone = Arc::clone(&template);

                    async move {
                        tracing::debug!(params = ?p_set, "building POST body");

                        // Replace the items in the template with matches from the group.
                        let body_init = template_clone.to_string();
                        let body = p_set
                            .iter()
                            .enumerate()
                            .fold(body_init, |body, (idx, repl)| {
                                let idx_thing = format!("${{{}}}", idx);
                                body.replace(&idx_thing, repl)
                            });
                        tracing::debug!(body = body);

                        let client = reqwest::Client::new();
                        let res = client
                            .post(ep_clone.as_str())
                            .header("X-Api-Key", key_clone.to_string())
                            .header("Content-Type", "application/json")
                            .body(body)
                            .send()
                            .await;

                        println!("{:#?}", res);
                    }
                })
            })
            .collect::<stream::FuturesUnordered<_>>();

        let result = futures::future::join_all(tasks).await;
        println!("{:?}", result);
    }
}
