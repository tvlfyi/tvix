package main

import (
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

func main() {
	log.Println("Running tvix-init…")

	log.Println("Creating /nix/store")
	os.MkdirAll("/nix/store", os.ModePerm)

	cmdline, err := os.ReadFile("/proc/cmdline")
	if err != nil {
		log.Printf("Failed to read cmdline: %s\n", err)
	}
	cmdlineFields := parseCmdline(string(cmdline))

	log.Println("Mounting…")
	if err := run("mount", "-t", "virtiofs", "tvix", "/nix/store", "-o", "ro"); err != nil {
		log.Printf("Failed to run mount: %v\n", err)
	}

	// If tvix.find is set, invoke find /nix/store
	if _, ok := cmdlineFields["tvix.find"]; ok {
		log.Println("Listing…")
		if err := run("find", "/nix/store"); err != nil {
			log.Printf("Failed to run find command: %s\n", err)
		}
	}

	// If tvix.shell is set, invoke the elvish shell
	if v, ok := cmdlineFields["tvix.shell"]; ok {
		log.Printf("Invoking shell%s\n…", v)
		if err := run("elvish"); err != nil {
			log.Printf("Failed to run shell: %s\n", err)
		}
	}

	// If tvix.exec is set, invoke the binary specified
	if v, ok := cmdlineFields["tvix.exec"]; ok {
		log.Printf("Invoking %s\n…", v)
		if err := syscall.Exec(v, []string{v}, []string{}); err != nil {
			log.Printf("Failed to exec: %s\n", err)
		}
	}

	log.Println("Powering off")
	if err := run("poweroff"); err != nil {
		log.Printf("Failed to run command: %v\n", err)
	}
}
