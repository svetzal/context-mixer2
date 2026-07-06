import { access, mkdir, readdir, readFile, rename, rm, stat, writeFile } from "node:fs/promises";

export interface DirEntry {
  fileName: string;
  isDirectory: boolean;
}

export interface Filesystem {
  exists(path: string): Promise<boolean>;
  read(path: string): Promise<Uint8Array>;
  readText(path: string): Promise<string>;
  write(path: string, content: string): Promise<void>;
  writeBytes(path: string, content: Uint8Array): Promise<void>;
  createDirAll(path: string): Promise<void>;
  removeDirAll(path: string): Promise<void>;
  rename(from: string, to: string): Promise<void>;
  listDir(path: string): Promise<DirEntry[]>;
  isDirectory(path: string): Promise<boolean>;
}

export interface Clock {
  now(): Date;
}

export class SystemClock implements Clock {
  public now(): Date {
    return new Date();
  }
}

export class NodeFilesystem implements Filesystem {
  public async exists(targetPath: string): Promise<boolean> {
    try {
      await access(targetPath);
      return true;
    } catch {
      return false;
    }
  }

  public async read(targetPath: string): Promise<Uint8Array> {
    return await readFile(targetPath);
  }

  public async readText(targetPath: string): Promise<string> {
    return await readFile(targetPath, "utf8");
  }

  public async write(targetPath: string, content: string): Promise<void> {
    await writeFile(targetPath, content, "utf8");
  }

  public async writeBytes(targetPath: string, content: Uint8Array): Promise<void> {
    await writeFile(targetPath, content);
  }

  public async createDirAll(targetPath: string): Promise<void> {
    await mkdir(targetPath, { recursive: true });
  }

  public async removeDirAll(targetPath: string): Promise<void> {
    await rm(targetPath, { recursive: true, force: true });
  }

  public async rename(from: string, to: string): Promise<void> {
    await rename(from, to);
  }

  public async listDir(targetPath: string): Promise<DirEntry[]> {
    const entries = await readdir(targetPath, { withFileTypes: true });

    return entries.map((entry) => ({
      fileName: entry.name,
      isDirectory: entry.isDirectory(),
    }));
  }

  public async isDirectory(targetPath: string): Promise<boolean> {
    try {
      return (await stat(targetPath)).isDirectory();
    } catch {
      return false;
    }
  }
}
