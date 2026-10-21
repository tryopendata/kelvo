import {
  remoteQueryText,
  sshTestCommand,
  sshVerificationCommand,
} from "./remote-errors";
import { CommandFailure } from "./transport/query";

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

it("words why a connected host did not answer a query apart from a dropped one", () => {
  const text = (
    kind: "remote_busy" | "remote_unsupported" | "remote_unreachable"
  ) =>
    remoteQueryText(new CommandFailure({ kind, message: "" }), "space reports");
  expect(text("remote_busy")).toBe(
    "This host is busy or slow to answer, so its space reports can't be read now. Try again in a moment."
  );
  expect(text("remote_unsupported")).toBe(
    "This host doesn't offer space reports."
  );
  expect(text("remote_unreachable")).toBe(
    "This host isn't connected, so its space reports can't be read now."
  );
  expect(remoteQueryText(new Error("other"), "space reports")).toBeNull();
});
