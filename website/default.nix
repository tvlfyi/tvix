{ depot, lib, pkgs, ... }:

let
  # https://developers.google.com/search/docs/advanced/structured-data/logo
  structuredData = {
    "@context" = "https://schema.org";
    "@type" = "Organisation";
    url = "https://tvl.fyi";
    logo = "https://static.tvl.fyi/latest/logo-animated.svg";
  };

  # All Tvix-related blog posts from the main TVL website
  tvixPosts = builtins.filter
    (post: !(post.draft or false) && (lib.hasInfix "Tvix" post.title))
    depot.web.tvl.blog.posts;

  postListEntries = map (p: "* [${p.title}](https://tvl.fyi/blog/${p.key})") tvixPosts;

  landing = depot.web.tvl.template {
    title = "Tvix - A new implementation of Nix";
    content = ''
      ${builtins.readFile ./landing-en.md}
      ${builtins.concatStringsSep "\n" postListEntries}
    '';
  };

in
pkgs.runCommand "tvix-website" { } ''
  mkdir $out
  cp ${landing} $out/index.html
  cp ${./tvix-logo.webp} $out/tvix-logo.webp
''
