import fs from "node:fs";

const source = fs.readFileSync(process.argv[2], "utf8");
const match = source.match(/return ([\s\S]*?<\/svg>';)/);
if (!match) {
  throw new Error("Could not find the IconPark SVG return expression");
}

const expression = match[1].replace(/;\s*$/, "");
const props = {
  size: 24,
  strokeWidth: 1.8,
  strokeLinecap: "round",
  strokeLinejoin: "round",
  colors: ["currentColor", "currentColor", "currentColor", "currentColor"],
};
const render = new Function("props", `return ${expression};`);
process.stdout.write(render(props));
