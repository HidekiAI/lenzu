import * as esbuild from 'esbuild';
import { copyFileSync, mkdirSync } from 'fs';

mkdirSync('dist/renderer', { recursive: true });

await esbuild.build({
  entryPoints: ['src/main.ts'],
  bundle: true,
  platform: 'node',
  external: ['electron'],
  outfile: 'dist/main.js',
});

await esbuild.build({
  entryPoints: ['src/preload.ts'],
  bundle: true,
  platform: 'node',
  external: ['electron'],
  outfile: 'dist/preload.js',
});

await esbuild.build({
  entryPoints: ['src/renderer/app.ts'],
  bundle: true,
  platform: 'browser',
  outfile: 'dist/renderer/app.js',
});

copyFileSync('src/renderer/index.html', 'dist/renderer/index.html');
copyFileSync('src/renderer/styles.css', 'dist/renderer/styles.css');

console.log('Build complete.');
