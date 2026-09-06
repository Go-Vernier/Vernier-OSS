import path from "node:path";
import { findDirs } from "./fs";
import { detectManifest } from "./language";

/**
 * Every directory up to four levels deep, so a declared service can be
 * matched to the directory holding its code when the declaration does not
 * say: `cartservice` -> `src/cartservice`, or the image
 * `springcommunity/spring-petclinic-vets-service` -> `spring-petclinic-vets-service`.
 *
 * Tiers, strongest first: exact basename, normalised basename, then a
 * basename that ends with the name after a separator. Shallower directories
 * win ties, and only directories with a manifest count.
 */
export class DirectoryIndex {
  private constructor(
    private readonly root: string,
    private readonly dirs: string[],
  ) {}

  static async build(root: string): Promise<DirectoryIndex> {
    const dirs = await findDirs(root, ["*", "*/*", "*/*/*", "*/*/*/*"]);
    dirs.sort((a, b) => depth(a) - depth(b) || a.localeCompare(b));
    return new DirectoryIndex(root, dirs);
  }

  /** Repository-relative directory for the first name that matches. */
  async match(names: Array<string | undefined>): Promise<string | null> {
    const wanted = [...new Set(names.filter((n): n is string => Boolean(n)))];
    const tiers: string[][] = [[], [], []];
    for (const dir of this.dirs) {
      const base = path.posix.basename(dir).toLowerCase();
      for (const name of wanted) {
        const lower = name.toLowerCase();
        if (base === lower) tiers[0]?.push(dir);
        else if (normalise(name) !== "" && normalise(base) === normalise(name)) tiers[1]?.push(dir);
        else if (lower.length >= 4 && /[-_.]/.test(base.slice(-lower.length - 1, -lower.length)) && base.endsWith(lower)) {
          tiers[2]?.push(dir);
        }
      }
    }
    for (const candidates of tiers) {
      for (const dir of candidates) {
        if (await detectManifest(path.join(this.root, dir))) return dir;
      }
    }
    return null;
  }
}

const depth = (p: string): number => p.split("/").length;

/** `checkout-api`, `checkout_svc`, `CheckoutService` all become `checkout`. */
export function normalise(s: string): string {
  return s
    .toLowerCase()
    .replace(/[-_.]/g, "")
    .replace(/(service|svc|api|server|deployment|deploy)$/, "");
}

/** `gcr.io/demo/cartservice:v1` -> `cartservice`; `redis:7-alpine` -> `redis`. */
export function imageBasename(image: string | undefined): string | undefined {
  if (!image) return undefined;
  const last = image.split("/").pop() ?? image;
  const name = last.split("@")[0]?.split(":")[0] ?? last;
  return name.length > 0 ? name : undefined;
}
