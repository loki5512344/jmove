import { used, unused } from './lib';
import './side-effects';
import Logger from './logger'; // used below, keep it
import type { Ghost } from './types';

export function run(): number {
  const log = new Logger();
  log.write('hello');
  return used(1, 2);
}
