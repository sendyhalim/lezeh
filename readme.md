# Install
### Building manually
```bash
make install
```

# Usage
###
```bash
# This merge-all command will
# 1. Make sure your local master and remote branch is updated
# 2. For all remote branches that contains the given branch names:
#    - Create local branch
#    - Rebase onto master
#    - Merge the local branch to master
#
# Note: you do not need to give full branch name, it will match by substring.
lezeh deployment merge-all <branch> <branch>
```
