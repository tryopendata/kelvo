/** Runtime-independent UI boundary. Adapters are loaded by app-transport. */
export type * from "./transport/contracts";
export { CommandFailure, unwrap } from "./transport/query";
