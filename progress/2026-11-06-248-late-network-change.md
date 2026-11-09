# A network change reported late is loaded again (#248)

Master's run 42164847174 (eeab765) failed the invite test at its first console page: "interrupted by another navigation to chrome-error://chromewebdata/", with no request failure named. `goto.mjs` now waits briefly for that failure, and loads again on a network change or when no cause is named. `admin_sign_in.rs` `a_network_change_shown_as_chromes_error_page_is_loaded_again` gains a late failure, a late refusal and a missing failure. It fails without the fix.
