use crate::{
    config::{ReleaseEndpoint, ReleaseProvider},
    error::Error,
    gitea::{Gitea, RemoteRelease},
    github::Github,
};
use reqwest::blocking::Client;
use std::{collections::BTreeMap, path::Path};

pub enum ReleaseClient {
    Gitea(Gitea),
    Github(Github),
}

impl ReleaseClient {
    pub fn new(endpoint: &ReleaseEndpoint, token: &str) -> Result<Self, Error> {
        match endpoint.provider {
            ReleaseProvider::Gitea => {
                Gitea::with_token(&endpoint.repository, token).map(Self::Gitea)
            }
            ReleaseProvider::Github => {
                Github::with_token(&endpoint.repository, token).map(Self::Github)
            }
        }
    }

    pub fn ensure_release(
        &self,
        tag: &str,
        commit: &str,
        draft: bool,
        prerelease: bool,
        message: Option<&str>,
        replace_existing_message: bool,
    ) -> Result<RemoteRelease, Error> {
        match self {
            Self::Gitea(api) => api.ensure_release(
                tag,
                commit,
                draft,
                prerelease,
                message,
                replace_existing_message,
            ),
            Self::Github(api) => api.ensure_release(
                tag,
                commit,
                draft,
                prerelease,
                message,
                replace_existing_message,
            ),
        }
    }

    pub fn repository_private(&self) -> Result<bool, Error> {
        match self {
            Self::Gitea(api) => api.repository_private(),
            Self::Github(api) => api.repository_private(),
        }
    }

    pub fn upload(&self, release_id: u64, paths: &[&Path], replace: bool) -> Result<(), Error> {
        match self {
            Self::Gitea(api) => api.upload(release_id, paths, replace),
            Self::Github(api) => api.upload(release_id, paths, replace),
        }
    }

    pub fn download_client(&self) -> &Client {
        match self {
            Self::Gitea(api) => api.download_client(),
            Self::Github(api) => api.download_client(),
        }
    }

    pub fn asset_urls(&self, release_id: u64) -> Result<Option<BTreeMap<String, String>>, Error> {
        match self {
            Self::Gitea(_) => Ok(None),
            Self::Github(api) => api.asset_urls(release_id).map(Some),
        }
    }
}
