use crate::models::{Comment, RawComment, Story};
use futures::{future::join_all, AsyncReadExt as _};
use gpui::http_client::{AsyncBody, HttpClient};
use std::collections::HashMap;
use std::sync::Arc;

const BASE_URL: &str = "https://hacker-news.firebaseio.com/v0";
const MAX_COMMENT_DEPTH: usize = 3;
const MAX_COMMENTS_PER_LEVEL: usize = 10;

#[derive(Clone)]
pub struct HackerNewsClient {
    client: Arc<dyn HttpClient>,
}

impl HackerNewsClient {
    pub fn new(client: Arc<dyn HttpClient>) -> Self {
        Self { client }
    }

    async fn get_json<T>(&self, url: &str) -> Result<T, String>
    where
        T: serde::de::DeserializeOwned + Send + 'static,
    {
        let response = self
            .client
            .get(url, AsyncBody::empty(), true)
            .await
            .map_err(|e| e.to_string())?;

        if !response.status().is_success() {
            return Err(format!("HTTP {} for {}", response.status(), url));
        }

        let mut body = response.into_body();
        let mut bytes = Vec::new();
        body.read_to_end(&mut bytes)
            .await
            .map_err(|e| e.to_string())?;

        serde_json::from_slice(&bytes).map_err(|e| e.to_string())
    }

    async fn fetch_item<T>(&self, id: i64) -> Option<T>
    where
        T: serde::de::DeserializeOwned + Send + 'static,
    {
        let url = format!("{}/item/{}.json", BASE_URL, id);
        self.get_json(&url).await.ok()
    }

    pub async fn fetch_top_stories(&self, limit: usize) -> Result<Vec<Story>, String> {
        let url = format!("{}/topstories.json", BASE_URL);
        let ids: Vec<i64> = self.get_json(&url).await?;

        let ids: Vec<i64> = ids.into_iter().take(limit).collect();

        // 并发获取所有 stories
        let futures: Vec<_> = ids.iter().map(|&id| self.fetch_item::<Story>(id)).collect();
        let results = join_all(futures).await;

        let mut stories: Vec<Story> = results.into_iter().flatten().collect();
        stories.sort_by(|a, b| b.score.cmp(&a.score));
        Ok(stories)
    }

    pub async fn fetch_comments(&self, story: &Story) -> Result<Vec<Comment>, String> {
        let kids = match &story.kids {
            Some(kids) => kids.clone(),
            None => return Ok(Vec::new()),
        };

        // 限制顶级评论数量
        let kids: Vec<i64> = kids.into_iter().take(MAX_COMMENTS_PER_LEVEL).collect();

        // 递归获取评论
        let comments = self.fetch_comments_recursive(&kids, 0).await;

        // 按树形结构排序
        let sorted = self.sort_comments_tree(&comments, &kids);
        Ok(sorted)
    }

    async fn fetch_comments_recursive(&self, ids: &[i64], depth: usize) -> Vec<Comment> {
        if depth > MAX_COMMENT_DEPTH || ids.is_empty() {
            return Vec::new();
        }

        // 限制每层评论数量
        let ids: Vec<i64> = ids.iter().take(MAX_COMMENTS_PER_LEVEL).copied().collect();

        // 并发获取当前层的所有评论
        let futures: Vec<_> = ids
            .iter()
            .map(|&id| self.fetch_item::<RawComment>(id))
            .collect();
        let results = join_all(futures).await;

        let mut comments = Vec::new();
        let mut all_kid_ids: Vec<Vec<i64>> = Vec::new();

        for raw in results.into_iter().flatten() {
            if raw.by.is_some() {
                let kids = raw.kids.clone();
                let reply_count = kids.as_ref().map_or(0, |k| k.len());
                let comment = Comment::from(raw).with_depth(depth);

                comments.push(Comment {
                    reply_count,
                    ..comment
                });

                // 收集子评论 IDs
                if let Some(kid_ids) = kids {
                    if !kid_ids.is_empty() {
                        all_kid_ids.push(kid_ids);
                    }
                }
            }
        }

        // 并发获取所有子评论
        let child_futures: Vec<_> = all_kid_ids
            .iter()
            .map(|kid_ids| self.fetch_comments_recursive(kid_ids, depth + 1))
            .collect();
        let child_results = join_all(child_futures).await;

        for child_comments in child_results {
            comments.extend(child_comments);
        }

        comments
    }

    /// 将扁平的评论列表按树形结构排序
    fn sort_comments_tree(&self, comments: &[Comment], root_ids: &[i64]) -> Vec<Comment> {
        // 建立 id -> comment 的映射
        let comment_map: HashMap<i64, &Comment> = comments.iter().map(|c| (c.id, c)).collect();

        // 建立 parent -> children 的映射
        let mut children_map: HashMap<i64, Vec<i64>> = HashMap::new();
        for c in comments {
            if let Some(kids) = &c.kids {
                children_map.insert(c.id, kids.clone());
            }
        }

        let mut result = Vec::new();

        // 从根节点开始深度优先遍历
        for &root_id in root_ids {
            self.collect_comments_dfs(root_id, &comment_map, &children_map, &mut result);
        }

        result
    }

    fn collect_comments_dfs(
        &self,
        id: i64,
        comment_map: &HashMap<i64, &Comment>,
        children_map: &HashMap<i64, Vec<i64>>,
        result: &mut Vec<Comment>,
    ) {
        if let Some(&comment) = comment_map.get(&id) {
            result.push(comment.clone());

            // 递归处理子评论
            if let Some(kids) = children_map.get(&id) {
                for &kid_id in kids {
                    self.collect_comments_dfs(kid_id, comment_map, children_map, result);
                }
            }
        }
    }
}
