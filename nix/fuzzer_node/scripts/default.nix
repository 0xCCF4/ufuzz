{pkgs}:
let
    file_names = builtins.attrNames (builtins.readDir ./.);
    files = builtins.map (file: ./${file}) file_names;

    removeSuffix = file: (builtins.replaceStrings [ ".sh" ] [ "" ] file);

    shellScripts = builtins.map (file: 
    pkgs.writeShellApplication {
            name = removeSuffix (baseNameOf file);
            text = builtins.readFile file;
        }
    ) (builtins.filter (file: pkgs.lib.hasSuffix ".sh" file) files);
in
shellScripts