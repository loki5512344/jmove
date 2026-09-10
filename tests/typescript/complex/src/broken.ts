// Fixture with one intentionally broken relative import.

export const flag = true;
import { ghost } from "./gone";

export const who = ghost;
