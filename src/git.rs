use std::path::Path;

pub trait GitManagement {
    fn init(&mut self, repo_path: &str) -> Result<(), git2::Error>;
    fn checkout_branch(&self, branch_name: &str) -> Result<(), git2::Error>;
    fn add(&self) -> Result<(), git2::Error>;
    fn commit(&self, subject: &str) -> Result<git2::Oid, git2::Error>;
    fn push(&self, branch_name: &str) -> Result<(), git2::Error>;
}

#[derive(Default)]
pub struct Git {
    repo: Option<git2::Repository>,
    ssh_key: String,
}

impl Git {
    pub fn new(ssh_key: &str) -> Self {
        Self {
            repo: None,
            ssh_key: ssh_key.to_owned(),
        }
    }
}

impl GitManagement for Git {
    fn init(&mut self, repo_path: &str) -> Result<(), git2::Error> {
        git2::Repository::open(Path::new(&repo_path)).map(|repo| self.repo = Some(repo))
    }

    fn checkout_branch(&self, branch_name: &str) -> Result<(), git2::Error> {
        let repo = self.repo.as_ref().unwrap();

        let commit = repo
            .head()
            .map(|head| head.target())
            .and_then(|oid| repo.find_commit(oid.unwrap()))?;

        // Create new branch if it doesn't exist
        match repo.branch(branch_name, &commit, false) {
            // This command can fail due to an existing reference. This error should be ignored.
            Err(err)
                if !(err.class() == git2::ErrorClass::Reference
                    && err.code() == git2::ErrorCode::Exists) =>
            {
                return Err(err);
            }
            _ => {}
        }

        let refname = format!("refs/heads/{}", branch_name);
        let obj = repo.revparse_single(refname.as_str())?;

        repo.checkout_tree(&obj, None)?;
        repo.set_head(refname.as_str())
    }

    fn add(&self) -> Result<(), git2::Error> {
        let mut index = self.repo.as_ref().unwrap().index()?;

        index.add_path(Path::new("README.md"))?;
        index.write()
    }

    fn commit(&self, subject: &str) -> Result<git2::Oid, git2::Error> {
        let repo = self.repo.as_ref().unwrap();
        let mut index = repo.index()?;

        let signature = repo.signature()?; // Use default user.name and user.email

        let oid = index.write_tree()?;
        let parent_commit = find_last_commit(self.repo.as_ref().unwrap())?;
        let tree = repo.find_tree(oid)?;

        repo.commit(
            Some("HEAD"),      // point HEAD to our new commit
            &signature,        // author
            &signature,        // committer
            subject,           // commit message
            &tree,             // tree
            &[&parent_commit], // parent commit
        )
    }

    fn push(&self, branch_name: &str) -> Result<(), git2::Error> {
        with_credentials(
            self.repo.as_ref().unwrap(),
            &self.ssh_key,
            |cred_callback| {
                let mut remote = self.repo.as_ref().unwrap().find_remote("origin")?;

                let mut callbacks = git2::RemoteCallbacks::new();
                let mut options = git2::PushOptions::new();

                callbacks.credentials(cred_callback);
                options.remote_callbacks(callbacks);

                remote.push(
                    &[format!(
                        "refs/heads/{}:refs/heads/{}",
                        branch_name, branch_name
                    )],
                    Some(&mut options),
                )?;

                Ok(())
            },
        )
    }
}

fn find_last_commit(repo: &git2::Repository) -> Result<git2::Commit, git2::Error> {
    let obj = repo.head()?.resolve()?.peel(git2::ObjectType::Commit)?;
    obj.into_commit()
        .map_err(|_| git2::Error::from_str("Couldn't find commit"))
}

/// Helper to run git operations that require authentication.
///
/// This is inspired by [the way Cargo handles this][cargo-impl].
///
/// [cargo-impl]: https://github.com/rust-lang/cargo/blob/94bf4781d0bbd266abe966c6fe1512bb1725d368/src/cargo/sources/git/utils.rs#L437
fn with_credentials<F>(repo: &git2::Repository, ssh_key: &str, mut f: F) -> Result<(), git2::Error>
where
    F: FnMut(&mut git2::Credentials) -> Result<(), git2::Error>,
{
    let config = repo.config()?;

    let mut tried_sshkey = false;
    let mut tried_cred_helper = false;
    let mut tried_default = false;

    f(&mut |url, username, allowed| {
        if allowed.contains(git2::CredentialType::USERNAME) {
            return Err(git2::Error::from_str("No username specified in remote URL"));
        }

        if allowed.contains(git2::CredentialType::SSH_KEY) && !tried_sshkey {
            tried_sshkey = true;
            let username = username.unwrap();
            let path = Path::new(ssh_key);
            return git2::Cred::ssh_key(username, None, path, None);
        }

        if allowed.contains(git2::CredentialType::USER_PASS_PLAINTEXT) && !tried_cred_helper {
            tried_cred_helper = true;
            return git2::Cred::credential_helper(&config, url, username);
        }

        if allowed.contains(git2::CredentialType::DEFAULT) && !tried_default {
            tried_default = true;
            return git2::Cred::default();
        }

        Err(git2::Error::from_str("No authentication method succeeded"))
    })
}

#[allow(non_snake_case)]
#[cfg(test)]
mod tests {
    use crate::git::{find_last_commit, Git, GitManagement};
    use git2::{BranchType, Repository, RepositoryInitOptions, Status};
    use tempfile::{NamedTempFile, TempDir};

    #[test]
    fn test_git__init__valid_repo() {
        let mut git = Git::default();
        // Valid repo
        let (dir, _repo, _file) = repo_init();

        let actual = git.init(dir.path().to_str().unwrap());

        assert!(actual.is_ok());
    }

    #[test]
    fn test_git__init__invalid_repo() {
        let mut git = Git::default();
        // Invalid repo
        let dir = TempDir::new().unwrap();

        let actual = git.init(dir.path().to_str().unwrap());

        assert!(actual.is_err());
    }

    #[test]
    fn test_git__checkout_branch__missing_branch() {
        let mut git = Git::default();
        let (dir, repo, _file) = repo_init();
        git.init(dir.path().to_str().unwrap()).unwrap();

        // This will create a new branch
        git.checkout_branch("new-branch-name").unwrap();

        let actual = repo.find_branch("new-branch-name", BranchType::Local);

        assert!(actual.is_ok());
    }

    #[test]
    fn test_git__checkout_branch__success() {
        let mut git = Git::default();
        let (dir, repo, _file) = repo_init();
        git.init(dir.path().to_str().unwrap()).unwrap();

        let before = repo.head();
        assert_eq!(before.unwrap().name().unwrap(), "refs/heads/main");

        git.checkout_branch("new-branch-name").unwrap();

        let after = repo.head();

        assert!(after.is_ok());
        assert_eq!(after.unwrap().name().unwrap(), "refs/heads/new-branch-name");
    }

    #[test]
    fn test_git__add__success() {
        let mut git = Git::default();
        let (dir, repo, _file) = repo_init();
        git.init(dir.path().to_str().unwrap()).unwrap();

        let statuses_before = repo.statuses(None).unwrap();
        let before = statuses_before.get(0).unwrap();
        assert_eq!(before.status(), Status::WT_NEW);

        git.add().unwrap();

        let statuses_after = repo.statuses(None).unwrap();
        let after = statuses_after.get(0).unwrap();
        assert_eq!(after.status(), Status::INDEX_NEW);
    }

    #[test]
    fn test_git__commit__success() {
        let mut git = Git::default();
        let (dir, _repo, _file) = repo_init();
        git.init(dir.path().to_str().unwrap()).unwrap();

        // Initial commit
        let before = find_last_commit(git.repo.as_ref().unwrap());
        assert_eq!(before.unwrap().summary().unwrap(), "initial-msg");

        git.add().unwrap();
        git.commit("some-subject").unwrap();

        let after = find_last_commit(git.repo.as_ref().unwrap());
        assert_eq!(after.unwrap().summary().unwrap(), "some-subject");
    }

    fn repo_init() -> (TempDir, Repository, NamedTempFile) {
        let td = TempDir::new().unwrap();
        let mut opts = RepositoryInitOptions::new();
        opts.initial_head("main");
        let repo = Repository::init_opts(td.path(), &opts).unwrap();

        // Create README.md file
        let file = tempfile::Builder::new()
            .prefix("README")
            .suffix(".md")
            .rand_bytes(0)
            .tempfile_in(td.path())
            .unwrap();
        {
            // Set basic config
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "some-name").unwrap();
            config.set_str("user.email", "some-email").unwrap();

            // Make initial commit
            let mut index = repo.index().unwrap();
            let id = index.write_tree().unwrap();
            let tree = repo.find_tree(id).unwrap();
            let sig = repo.signature().unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "initial-msg", &tree, &[])
                .unwrap();
        }
        // Return file to not drop it and make it disappear
        (td, repo, file)
    }
}
