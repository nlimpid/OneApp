use crate::models::{Comment, Story};

const BASE_URL: &str = "https://hacker-news.firebaseio.com/v0";

pub struct HackerNewsClient {
    agent: ureq::Agent,
}

impl Default for HackerNewsClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HackerNewsClient {
    pub fn new() -> Self {
        Self {
            agent: ureq::Agent::new(),
        }
    }

    pub fn fetch_top_stories(&self, limit: usize) -> Result<Vec<Story>, String> {
        let url = format!("{}/topstories.json", BASE_URL);
        let ids: Vec<i64> = self
            .agent
            .get(&url)
            .call()
            .map_err(|e| e.to_string())?
            .into_json()
            .map_err(|e| e.to_string())?;

        let ids: Vec<i64> = ids.into_iter().take(limit).collect();

        let mut stories = Vec::new();
        for id in ids {
            let url = format!("{}/item/{}.json", BASE_URL, id);
            if let Ok(response) = self.agent.get(&url).call() {
                if let Ok(story) = response.into_json::<Story>() {
                    stories.push(story);
                }
            }
        }

        stories.sort_by(|a, b| b.score.cmp(&a.score));
        Ok(stories)
    }

    pub fn fetch_comments(&self, story: &Story) -> Result<Vec<Comment>, String> {
        let kids = match &story.kids {
            Some(kids) => kids.clone(),
            None => return Ok(Vec::new()),
        };

        let mut comments = Vec::new();
        for id in kids {
            let url = format!("{}/item/{}.json", BASE_URL, id);
            if let Ok(response) = self.agent.get(&url).call() {
                if let Ok(comment) = response.into_json::<Comment>() {
                    if comment.by.is_some() {
                        comments.push(comment);
                    }
                }
            }
        }

        Ok(comments)
    }
}
