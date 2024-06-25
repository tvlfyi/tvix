# Code & Commits

## Code quality

This one should go without saying — but please ensure that your code quality
does not fall below the rest of the project. This is of course very subjective,
but as an example if you place code that throws away errors into a block in
which errors are handled properly your change will be rejected.


```admonish hint
Usually there is a strong correlation between the visual appearance of a code
block and its quality. This is a simple way to sanity-check your work while
squinting and keeping some distance from your screen ;-)
```


## Commit messages

The [Angular Conventional Commits][angular] style is the general commit style
used in the Tvix project. Commit messages should be structured like this:

```admonish example
    type(scope): Subject line with at most a 72 character length

    Body of the commit message with an empty line between subject and
    body. This text should explain what the change does and why it has
    been made, *especially* if it introduces a new feature.

    Relevant issues should be mentioned if they exist.
```

Where `type` can be one of:

* `feat`: A new feature has been introduced
* `fix`: An issue of some kind has been fixed
* `docs`: Documentation or comments have been updated
* `style`: Formatting changes only
* `refactor`: Hopefully self-explanatory!
* `test`: Added missing tests / fixed tests
* `chore`: Maintenance work
* `subtree`: Operations involving `git subtree`

And `scope` should refer to some kind of logical grouping inside of the
project.

It does not make sense to include the full path unless it aids in
disambiguating. For example, when changing the struct fields in
`tvix/glue/src/builtins/fetchers.rs` it is enough to write
`refactor(tvix/glue): …`.

Please take a look at the existing commit log for examples.


## Commit content

Multiple changes should be divided into multiple git commits whenever possible.
Common sense applies.

The fix for a single-line whitespace issue is fine to include in a different
commit. Introducing a new feature and refactoring (unrelated) code in the same
commit is not fine.

`git commit -a` is generally **taboo**, whereas on the command line you should
be preferring `git commit -p`.


```admonish tip
Tooling can really help this process. The [lazygit][] TUI or [magit][] for
Emacs are worth looking into.
```


[angular]: https://www.conventionalcommits.org/en/
[lazygit]: https://github.com/jesseduffield/lazygit
[magit]: https://magit.vc
