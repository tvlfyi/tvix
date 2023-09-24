package main

import (
	"fmt"
	"log"
	"os"
	"os/exec"
	"strings"
	"syscall"
)

// run the given command, connecting std{in,err,out} with the OS one.
func run(args ...string) error {
	cmd := exec.Command(args[0], args[1:]...)
	cmd.Stdin = os.Stdin
	cmd.Stderr = os.Stderr
	cmd.Stdout = os.Stdout

	return cmd.Run()
}

// parse the cmdline, return a map[string]string.
func parseCmdline(cmdline string) map[string]string {
	line := strings.TrimSuffix(cmdline, "\n")
	fields := strings.Fields(line)
	out := make(map[string]string, 0)

	for _, arg := range fields {
		kv := strings.SplitN(arg, "=", 2)
		switch len(kv) {
		case 1:
			out[kv[0]] = ""
		case 2:
			out[kv[0]] = kv[1]
		}
	}

	return out
}

// mounts the nix store from the virtiofs tag to the given destination,
// creating the destination if it doesn't exist already.
func mountTvixStore(dest string) error {
	if err := os.MkdirAll(dest, os.ModePerm); err != nil {
		return fmt.Errorf("unable to mkdir dest: %w", err)
	}
	if err := run("mount", "-t", "virtiofs", "tvix", dest, "-o", "ro"); err != nil {
		return fmt.Errorf("unable to run mount: %w", err)
	}

	return nil
}

func main() {
	fmt.Print(`
  ______      _         ____      _ __
 /_  __/   __(_)  __   /  _/___  (_) /_
  / / | | / / / |/_/   / // __ \/ / __/
 / /  | |/ / />  <   _/ // / / / / /_
/_/   |___/_/_/|_|  /___/_/ /_/_/\__/

`)

	// Set PATH to "/bbin", so we can find the u-root tools
	os.Setenv("PATH", "/bbin")

	if err := run("mount", "-t", "proc", "none", "/proc"); err != nil {
		log.Printf("Failed to mount /proc: %v\n", err)
	}
	if err := run("mount", "-t", "sysfs", "none", "/sys"); err != nil {
		log.Printf("Failed to mount /sys: %v\n", err)
	}
	if err := run("mount", "-t", "devtmpfs", "devtmpfs", "/dev"); err != nil {
		log.Printf("Failed to mount /dev: %v\n", err)
	}

	cmdline, err := os.ReadFile("/proc/cmdline")
	if err != nil {
		log.Printf("Failed to read cmdline: %s\n", err)
	}
	cmdlineFields := parseCmdline(string(cmdline))

	if _, ok := cmdlineFields["tvix.find"]; ok {
		// If tvix.find is set, invoke find /nix/store
		if err := mountTvixStore("/nix/store"); err != nil {
			log.Printf("Failed to mount tvix store: %v\n", err)
		}

		if err := run("find", "/nix/store"); err != nil {
			log.Printf("Failed to run find command: %s\n", err)
		}
	} else if _, ok := cmdlineFields["tvix.shell"]; ok {
		// If tvix.shell is set, mount the nix store to /nix/store directly,
		// then invoke the elvish shell
		if err := mountTvixStore("/nix/store"); err != nil {
			log.Printf("Failed to mount tvix store: %v\n", err)
		}

		if err := run("elvish"); err != nil {
			log.Printf("Failed to run shell: %s\n", err)
		}
	} else if v, ok := cmdlineFields["tvix.run"]; ok {
		// If tvix.run is set, mount the nix store to /nix/store directly,
		// then invoke the command.
		if err := mountTvixStore("/nix/store"); err != nil {
			log.Printf("Failed to mount tvix store: %v\n", err)
		}

		if err := run(v); err != nil {
			log.Printf("Failed to run command: %s\n", err)
		}
	} else if v, ok := cmdlineFields["init"]; ok {
		// If init is set, invoke the binary specified (with switch_root),
		// and prepare /fs beforehand as well.
		os.Mkdir("/fs", os.ModePerm)
		if err := run("mount", "-t", "tmpfs", "none", "/fs"); err != nil {
			log.Fatalf("Failed to mount /fs tmpfs: %s\n", err)
		}

		// Mount /fs/nix/store
		if err := mountTvixStore("/fs/nix/store"); err != nil {
			log.Fatalf("Failed to mount tvix store: %v\n", err)
		}

		// Invoke switch_root, which will take care of moving /proc, /sys and /dev.
		if err := syscall.Exec("/bbin/switch_root", []string{"switch_root", "/fs", v}, []string{}); err != nil {
			log.Printf("Failed to switch root: %s\n", err)
		}
	} else {
		log.Printf("No command detected, not knowing what to do!")
	}

	// This is only reached in the non switch_root case.
	log.Printf("Nothing left to be done, powering off.")
	if err := run("poweroff"); err != nil {
		log.Printf("Failed to run poweroff command: %v\n", err)
	}
}
