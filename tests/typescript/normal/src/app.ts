import type { Thing } from "./types";
import express from "express";
import "./impl/core";
import { log } from "./utils/logger";

const cfg = require("./types");
const later = () => import("./types");

export const app = express();
export function render(t: Thing): string {
  log(cfg.name, t.id);
  return t.id;
}
export { later };
