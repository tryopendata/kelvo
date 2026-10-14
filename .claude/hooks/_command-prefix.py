#!/usr/bin/env python3
"""Unwrap familiar launchers for approval classification; never evaluate shell code.

This is deliberately a prefix recognizer, not a shell security boundary. The
shared destructive scan, worktree isolation and Git history remain backstops.
Unknown inner commands use normal permissions; interpreter scripts remain dev
commands. Shell operators are screened by the caller before this runs.
"""
import re
import shlex
import sys

ASSIGNMENT = re.compile(r"[A-Za-z_][A-Za-z_0-9]*=")
OPTIONS_WITH_VALUE = {
    "env": {"-u", "--unset", "-C", "--chdir", "-a", "--argv0"},
    "timeout": {"-k", "--kill-after", "-s", "--signal"},
    "time": {"-o", "--output", "-f", "--format"},
    "xargs": {"-E", "-I", "-J", "-L", "-n", "-P", "-R", "-S", "-s", "-d",
              "--eof", "--replace", "--max-lines", "--max-args", "--max-procs",
              "--max-chars", "--delimiter", "--arg-file", "-a"},
    "nohup": set(),
    "command": set(),
}


def unwrap(words):
    while words:
        while words and ASSIGNMENT.match(words[0]):
            words = words[1:]
        if not words or words[0] not in OPTIONS_WITH_VALUE:
            return words
        wrapper = words[0]
        index = 1
        while index < len(words) and words[index].startswith("-"):
            option = words[index]
            index += 1
            if option == "--":
                break
            if wrapper == "env" and option in {"-S", "--split-string"}:
                if index == len(words):
                    return words
                words = ["env", *shlex.split(words[index]), *words[index + 1:]]
                index = 1
                continue
            if wrapper == "env" and option.startswith("--split-string="):
                words = ["env", *shlex.split(option.partition("=")[2]), *words[index:]]
                index = 1
                continue
            if option in OPTIONS_WITH_VALUE[wrapper]:
                index += 1
        if wrapper == "env":
            while index < len(words) and ASSIGNMENT.match(words[index]):
                index += 1
            if index == len(words):
                return ["env"]  # print the environment with assignments applied
        if wrapper == "timeout":
            index += 1  # duration (including suffixes such as 120s)
        # Bare env/time/xargs retain their existing utility classification.
        if index >= len(words):
            return words
        words = words[index:]
    return words


if __name__ == "__main__":
    command = sys.stdin.read()
    try:
        print(shlex.join(unwrap(shlex.split(command))))
    except ValueError:
        # Incomplete quoting is left to the shell/harness, never evaluated here.
        print(command)
