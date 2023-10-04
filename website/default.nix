{ depot, lib, pkgs, ... }:

let
  description = "Rust implementation of the purely-functional Nix package manager";

  # https://developers.google.com/search/docs/advanced/structured-data/
  # https://schema.org/SoftwareApplication
  structuredData = {
    "@context" = "https://schema.org";
    "@type" = "SoftwareApplication";
    name = "Tvix";
    url = "https://tvix.dev";
    abstract = description;
    applicationCategory = "DeveloperApplication";
    contributor = "https://tvl.fyi";
    image = "https://tvix.dev/logo.webp";
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

    extraHead = ''
      <meta name="description" content="${description}">
      <script type="application/ld+json">
        ${builtins.toJSON structuredData}
      </script>
    '';
  };

in
pkgs.runCommand "tvix-website" { } ''
  mkdir $out
  cp ${landing} $out/index.html
  cp ${depot.tvix.logo}/logo.webp $out/
''
