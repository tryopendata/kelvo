// Compile-only contract: unused @ts-expect-error fails if a global leaks in.
// @ts-expect-error Node globals do not belong in the browser program.
process.cwd();
// @ts-expect-error Buffer requires an explicit Node import, forbidden by Biome.
Buffer.from("browser");
// @ts-expect-error Test globals do not belong in application modules.
describe("browser", () => {});
// @ts-expect-error Test matchers do not belong in application modules.
expect(true).toBe(true);
// @ts-expect-error Environment keys must be declared in vite-env.d.ts.
import.meta.env.VITE_UNDECLARED;

// @ts-expect-error Exact optional properties distinguish omission from a value.
const omittedSetting: { enabled?: boolean } = { enabled: undefined };
void omittedSetting;
