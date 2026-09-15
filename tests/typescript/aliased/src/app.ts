import { shout } from "@utils/str";
import cfg from "@cfg";
import { log } from "./utils/log";

export function go(): string {
  return shout(cfg) + log();
}
