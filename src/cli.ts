#!/usr/bin/env node
import { Command } from "commander";
import pkg from "../package.json";
import { analysisToJSON, analyze } from "./analyze";
import { formatRepoReport } from "./report/terminal";

const program = new Command();

program
  .name("blast-radius")
  .description("Which services can this change reach?")
  .version(pkg.version);

program
  .command("analyze")
  .description("Analyse a repository: its services, and what a change can reach")
  .argument("[path]", "repository root", ".")
  .option("--json", "print the graph as JSON instead of the report")
  .option("--no-color", "disable colours")
  .action(async (target: string, opts: { json?: boolean; color: boolean }) => {
    try {
      const analysis = await analyze(target);
      if (opts.json) {
        process.stdout.write(`${JSON.stringify(analysisToJSON(analysis), null, 2)}\n`);
        return;
      }
      const color = opts.color && Boolean(process.stdout.isTTY) && !process.env.NO_COLOR;
      process.stdout.write(`${formatRepoReport(analysis, { color })}\n`);
    } catch (err) {
      process.stderr.write(`blast-radius: ${err instanceof Error ? err.message : String(err)}\n`);
      process.exitCode = 1;
    }
  });

await program.parseAsync(process.argv);
