# Contributing to Tvix

## Registration

Self-hosted [Gerrit](https://www.gerritcodereview.com) & changelists (CLs) are
the preferred method of contributions & review.

TVL’s Gerrit supports single sign-on (SSO) using a GitHub, GitLab, or
StackOverflow account.

Additionally if you would prefer not to use an SSO option or wish to have a
backup authentication strategy in the event of downed server or otherwise, we
recommend setting up a TVL-specific LDAP account.

You can create such an account by following these instructions:

1. Checkout [TVL’s monorepo][check-out-monorepo] if you haven’t already
2. Be a member of `#tvix-dev` (and/or `#tvl`) on [hackint][], a communication
   network.
3. Generate a user entry using [//web/pwcrypt](https://signup.tvl.fyi/).
4. Commit that generated user entry to our LDAP server configuration in
   [ops/users][ops-users] (for an example, see:
   [CL/2671](https://cl.tvl.fyi/c/depot/+/2671))
5. If only using LDAP, submit the patch via email (see [<cite>Submitting
   changes via email</cite>][email])


## Gerrit setup

Gerrit uses the concept of change IDs to track commits across rebases and other
operations that might change their hashes, and link them to unique changes in
Gerrit.

First, [upload your public SSH keys to Gerrit][Gerrit SSH]. Then change your
remote to point to your newly-registered user over SSH. Then follow up with Git
config by setting the default push URLs for & installing commit hooks for a
smoother Gerrit experience.

```console
$ cd depot
$ git remote set-url origin "ssh://$USER@code.tvl.fyi:29418/depot"
$ git config remote.origin.url "ssh://$USER@code.tvl.fyi:29418/depot"
$ git config remote.origin.push "HEAD:refs/for/canon"
$ curl -L --compressed https://cl.tvl.fyi/tools/hooks/commit-msg | tee .git/hooks/commit-msg
…
if ! mv "${dest}" "$1" ; then
  echo "cannot mv ${dest} to $1"
  exit 1
fi
$ chmod +x .git/hooks/commit-msg
```

## Gerrit workflow

The workflow on Gerrit is quite different than the pull request (PR) model that
many developers are more likely to be accustomed to. Instead of pushing changes
to remote branches, all changes have to be pushed to `refs/for/canon`. For each
commit that is pushed there, a change request is created automatically

Every time you create a new commit the change hook will insert a unique
`Change-Id` tag into the commit message. Once you are satisfied with the state
of your commit and want to submit it for review, you push it to a Git `ref`
called `refs/for/canon`. This designates the commits as changelists (CLs)
targeted for the `canon` branch.

After you feel satisfied with your changes changes, push to the default:

```console
$ git commit -m 'docs(REVIEWS): Fixed all the errors in the reviews docs'
$ git push origin
```

Or to a special target, such as a work-in-progress CL:

```console
$ git push origin HEAD:refs/for/canon%wip
```

During the review process, the reviewer(s) might ask you to make changes. You
can simply amend[^amend] your commit(s) then push to the same ref (`--force*`
flags not needed). Gerrit will automatically update your changes.

```admonish caution
Every individual commit will become a separate change. We do *not* squash
related commits, but instead submit them one by one. Be aware that if you are
expecting a different behavior such as attempt something like an unsquashed
subtree merge, you will produce a *lot* of CLs. This is strongly discouraged.
```

```admonish tip
If do not have experience with the Gerrit model, consider reading the
[<cite>Working with Gerrit: An example</cite>][Gerrit Walkthrough] or
[<cite>Basic Gerrit Walkthrough — For GitHub Users</cite>][github-diff].

It will also be important to read about [attention sets][] to understand how
your ‘turn’ works, how notifications will be distributed to users through the
system, as well as the other [attention set rules][attention-set-rules].
```


[check-out-monorepo]: ./getting-started#tvl-monorepo
[email]: ../contributing/email.html
[Gerrit SSH]: https://cl.tvl.fyi/settings/#SSHKeys
[Gerrit walkthrough]: https://gerrit-review.googlesource.com/Documentation/intro-gerrit-walkthrough.html
[ops-users]: https://code.tvl.fyi/tree/ops/users/default.nix
[hackint]: https://hackint.org
[github-diff]: https://gerrit.wikimedia.org/r/Documentation/intro-gerrit-walkthrough-github.html
[attention sets]: https://gerrit-review.googlesource.com/Documentation/user-attention-set.html
[attention-set-rules]: https://gerrit-review.googlesource.com/Documentation/user-attention-set.html#_rules
[^keycloak]: [^amend]: `git commit --amend`
