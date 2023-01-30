# Lezeh
`lezeh` is a CLI tool to ease day-to-day engineering operations such as:
* Cherry pick database row and its relations
* Visualize graph representation of a database row relations
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
| [`lezeh-x86_64-unknown-linux-gnu.zip`](https://github.com/sendyhalim/lezeh/releases/latest/download/lezeh-x86_64-unknown-linux-gnu.zip) | Linux |
| [`lezeh-x86_64-apple-darwin.zip`](https://github.com/sendyhalim/lezeh/releases/latest/download/lezeh-x86_64-apple-darwin.zip)      | macOS |

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
First create config file at `~/.lezeh`, we're using YAML format. Each top level key
is named after lezeh sub command:
* `url` maps to `lezeh url ...` command
* `db` maps to `lezeh db ...` sub command
* and so on...

```yaml
url:
  bitly:
    api_token: test123

db:
  db_connection_by_name:
    testdb:
      host: localhost
      port: 5432
      database: db_name
      username: ....
      password: ....

deployment:
  phab:
    api_token: test125
    pkcs12_path: /path/to/pkcs12
    host: 'yourphabricatorhost.com'
    pkcs12_password: abcdefg

  ghub:
    api_token: test124

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
### URL cli
```
lezeh url shorten {longUrl}
```

### Database cli
Mostly tooling related with database operations. Only supports postgres as of now.

#### cherry-pick
Imagine you have this 1 table row that you want to copy but you can't easily
copy it because it has relations and you need to copy the parents and children
recursively. This is where cherry-pick 🍒 can be useful, it will fetch row that matches the given column-value pair including its relations, then build a graph from it, the graph can be serialized into insert statements(default option) or graphviz(to visualize the graph)

```bash
lezeh db cherry-pick \
  # Fetch from test_db, this one is based on the config
  --source-db=testdb \

  # As of now only supports 1 value, but it will change in the future
  --values=123 \

  # Table that the value will be fetched from
  --table=orders \

  # [Optional] which column that contains the given values, defaults to id
  --column=id \

  # [Optional] Db schema, defaults to public
  --schema=public \

  # [Optional], defaults to insert-statement. If supplied Graphviz then it'll serialize
  # the graph representation that can be represented in a graphviz format
  # see https://graphviz.org/ for more details.
  # The output can be used on online graphviz visualizer:
  # * https://edotor.net
  # * https://dreampuf.github.io/GraphvizOnline
  --output-format=insert-statement|graphviz \

  # [Optional]
  # The option will be used if you choose pass `--output-format=graphviz`.
  # Set the table columns that will be displayed on each node, if not set it'll
  # default to only show the row id, format:
  # '{table_1}:{column_1}|{column_2}|{column_n},{table_n}:{column_n}'
  #
  # Suppose you pass `--graph-table-columns='users:id|name|email, orders:code'`, it will
  # * Show id, name and email column value for all fetched rows from users table
  # * Show code column value for all fetched rows from orders table
  # * The other rows from other tables will still only show row id because
  #   it's not overriden
  --graph-table-columns='{table_1}:{column_1}|{column_2}|{column_n},{table_n}:{column_n}, {table_n}:{column_n}'
```


### Deployment cli
```bash
# Below command will iterate all repositories under deployment.repositories config
# and do the following operations:
# * Make sure your local git data is updated by pulling remote git data from GH.
# * For each remote branches that contains the given task numbers:
#    - Print out phabricator task owner (assigned) for that specific branch.
#    - Create a PR for the matched branch.
#    - Merge the branch into master with SQUASH strategy
#    - Delete the remote branch
lezeh deployment merge-feature-branches \
  {task_number} {task_number} {task_number} ... \
  
  # Set number of concurrency limit when merging feature branchs. Defaults to 1, meaning it will 
  # merge feature branches per repository sequentially. If you set it to N then it will run in 
  # parallel at most N repositories at a time. At the repository level, merging should be sequential 
  # otherwise it'll race with other pull requests.
  --concurrency-limit=1


# Merge repo (given repo key) based on given deployment scheme config.
#
# Example usage (based on above config example):
# lezeh deployment deploy repo-key stg
lezeh deployment deploy {repo_key} {scheme_key}
```
