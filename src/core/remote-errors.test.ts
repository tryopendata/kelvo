import { sshTestCommand, sshVerificationCommand } from "./remote-errors";

it("keeps shell syntax inside a quoted SSH destination", () => {
  expect(sshTestCommand("o'brien@$(whoami)")).toBe(
    "/usr/bin/ssh -o BatchMode=yes -o StrictHostKeyChecking=yes -o ConnectTimeout=5 -o ControlPath=none -- 'o'\\''brien@$(whoami)' true"
  );
  for (const input of ["ssh studio", "-Fconfig", "studio\nother", ""]) {
    expect(sshTestCommand(input)).toBeNull();
  }
});

it("allows host confirmation only in the interactive verification command", () => {
  expect(sshVerificationCommand("o'brien@$(whoami)")).toBe(
    "/usr/bin/ssh -o BatchMode=no -o StrictHostKeyChecking=ask -o ConnectTimeout=5 -o ControlPath=none -- 'o'\\''brien@$(whoami)' true"
  );
  for (const input of ["ssh studio", "-Fconfig", "studio\nother", ""]) {
    expect(sshVerificationCommand(input)).toBeNull();
  }
});
