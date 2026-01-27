use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseRepoInfo {
    pub path: String,
    pub canonical_path: String,
    pub current_branch: String,
    pub head: String,
}
