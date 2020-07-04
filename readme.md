# Install
### Building manually
```bash
make install
```

# Setup
First create config file at `~/.lezeh`, we're [Hjson](https://hjson.github.io/) format.

```bash
{
  # As of now, you just need to set phab config,
  # please see https://github.com/sendyhalim/phab for more details
  phab: {
    api_token: ...,
    pkcs12_path: ...,
    host: yourphabricatorhost.com,
    pkcs12_password: ...,
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
