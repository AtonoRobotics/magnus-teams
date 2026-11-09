# Master's runs finish (2026-11-06)

- desktop.yml and injection.yml cancel an earlier run only for a pull request (`cancel-in-progress: ${{ github.event_name == 'pull_request' }}`). On master a new run waits for the one in progress, and GitHub keeps only the newest waiting run. With pull requests merging beside each other (#219), each merge had cancelled master's run, so master was never tested in full and the owner's app got no build. Test: `a_merge_does_not_cancel_masters_run`.
