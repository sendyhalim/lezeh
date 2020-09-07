# Install
### Download binaries
Go to latest releases and download the binaries [here](https://github.com/sendyhalim/lezeh/releases/latest)

| Binary                               | OS    |
| ------------------------------------ | ----- |
| `lezeh-x86_64-unknown-linux-gnu.zip` | Linux |
| `lezeh-x86_64-apple-darwin.zip`      | macOS |

### Building manually
```bash
make install
```


# Setup
First create config file at `~/.lezeh`, we're using [Hjson](https://hjson.github.io/) format.

```bash
{
  # As of now, you just need to set phab config,
  # please see https://github.com/sendyhalim/phab for more details
  phab: {
    api_token: ...
    pkcs12_path: ...
    host: https://yourphabricatorhost.com
    pkcs12_password: ...
  },
  ghub: {
    # This is your github personal token,
    # you need to register token with a full repository write access.
    api_token: abc123
  },

  # Deployment command config
  deployment: {
    repositories: [
      {
        # This is a unique key that will be used as hashmap key
        # for the repo.
        key: "repo-key"
        path: "repo-local-path"
        github_path: "username/reponame"


        # This config will be used when you're running
        # deploy command, example:
        # ```
        # lezeh deployment deploy repo-name <stg|prod>
        # ```
        deployment_scheme_by_key: {
          stg: {
            name: "Deploy to stg"
            default_pull_request_title: "Merge into stg"
            merge_from_branch: "master"
            merge_into_branch: "stg"
          }
          prod: {
            name: "Deploy to prod"
            default_pull_request_title: "Merge into prod"
            merge_from_branch: "stg"
            merge_into_branch: "prod"
          }
        }
      }
    ]
  }
}
```


# Usage
###
```bash
# Below command will
# 1. Make sure your local git data is updated by pulling remote git data from GH.
# 2. For each remote branches that contains the given task numbers:
#    - Print out phabricator task owner (assigned) for that specific branch.
#    - Create a PR for the matched branch.
#    - Merge the branch into master with SQUASH strategy
#    - Delete the remote branch
#
# Note: you do not need to give full branch name, it will match by substring.
lezeh deployment merge-feature-branches <task_number> <task_number> <...>


# Merge repo (given repo key) based on given deployment scheme config.
# Please see config (in the config example) explanation
# for key deployment.repositories.deployment_scheme_by_key
#
# Example usage (based on above config example):
# lezeh deployment deploy repo-key stg
lezeh deployment deploy <repo_key> <scheme_key>
```
