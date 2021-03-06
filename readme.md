# Lezeh
`lezeh` is a CLI tool to ease day-to-day engineering operations such as:
* Merging feature branch (by convention, specific using phabricator task number) into master,
  this includes cleaning up (delete) the merged feature branch
* Merge and run deployment commands

[![Crates.io](https://img.shields.io/crates/v/lezeh)](https://crates.io/crates/lezeh)
[![Crates.io](https://img.shields.io/crates/l/lezeh)](./license)


## Install
### Download binaries
Go to latest releases and download the binaries [here](https://github.com/sendyhalim/lezeh/releases/latest)

| Binary                               | OS    |
| ------------------------------------ | ----- |
| `lezeh-x86_64-unknown-linux-gnu.zip` | Linux |
| `lezeh-x86_64-apple-darwin.zip`      | macOS |

### Using cargo
```bash
cargo install lezeh
```

### Building manually
This requires [rust](https://www.rust-lang.org/tools/install)
```bash
make install
```


## Setup
First create config file at `~/.lezeh`, we're using YAML format.

```yaml
phab:
  api_token: test125
  pkcs12_path: /path/to/pkcs12
  host: 'yourphabricatorhost.com'
  pkcs12_password: abcdefg

ghub:
  api_token: test124

bitly:
  api_token: test123

# Deployment command config
deployment:
  repositories:
      # This is a unique key that will be used as hashmap key
      # for the repo.
    - key: "repo-key"
      path: "repo-local-path"
      github_path: "username/reponame"
      deployment_scheme_by_key:
        stg:
          name: "Deploy to stg"
          default_pull_request_title: "Merge into stg"
          merge_from_branch: "master"
          merge_into_branch: "stg"
        prod:
          name: "Deploy to prod"
          default_pull_request_title: "Merge into prod"
          merge_from_branch: "stg"
          merge_into_branch: "prod"
```


## Usage
### Deployment Command

```bash
# Below command will iterate all repositories under deployment.repositories config
# and do the following operations:
# * Make sure your local git data is updated by pulling remote git data from GH.
# * For each remote branches that contains the given task numbers:
#    - Print out phabricator task owner (assigned) for that specific branch.
#    - Create a PR for the matched branch.
#    - Merge the branch into master with SQUASH strategy
#    - Delete the remote branch
lezeh deployment merge-feature-branches {task_number} {task_number} {task_number} ...


# Merge repo (given repo key) based on given deployment scheme config.
#
# Example usage (based on above config example):
# lezeh deployment deploy repo-key stg
lezeh deployment deploy {repo_key} {scheme_key}
```

### URL Command
```
lezeh url shorten {longUrl}
```

