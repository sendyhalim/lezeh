# Install
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
        key: "repo-name"
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
# This merge-all command will
# 1. Make sure your local master and remote branch is updated
# 2. For all remote branches that contains the given branch names:
#    - Print out phabricator task owner (assigned) for that specific branch.
#    - Create local branch.
#    - Rebase onto master.
#    - Merge the local branch to master.
#
# Note: you do not need to give full branch name, it will match by substring.
lezeh deployment merge-all <branch> <branch>
```
