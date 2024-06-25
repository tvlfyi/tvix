# Submitting changes via email

With SSO & local accounts, hopefully Tvix provides you a low-friction or
privacy-respecting way to make contributions by means of
[TVL’s self-hosted Gerrit][gerrit]. However, if you still decide differently,
you may submit a patch via email to `depot@tvl.su` where it will be added to
Gerrit by a contributor.

Please keep in mind this process is more complicated requiring extra work from
both us & you:

* You will need to manually check the Gerrit website for updates & someone will
  need to relay potential comments to/from Gerrit to you as you won’t get
  emails from Gerrit.
* New revisions need to be stewarded by someone uploading changes to Gerrit
  on your behalf.
* As CLs cannot change owners, if you decide to get a Gerrit account later on
  existing CLs need to be abandoned then recreated. This introduces more churn
  to the review process since prior discussion are disconnected.

Create an appropriate commit locally then send it us using either of these
options:

* `git format-patch`: This will create a `*.patch` file which you should email to
  us.
* `git send-email`: If configured on your system, this will take care of the
  whole emailing process for you.

The email address is a [public inbox][].


[gerrit]: ../contributing/gerrit.html
[public inbox]: https://inbox.tvl.su/depot/
