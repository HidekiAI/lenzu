import * as esbuild from 'esbuild';
import { copyFileSync, mkdirSync } from 'fs';
import { execSync } from 'child_process';

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

// Compile the X11 override-redirect helper (Linux only).
// Requires libx11-dev: apt install libx11-dev
try {
  execSync(
    'gcc -O2 -o dist/hud-set-override-redirect' +
    ' scripts/hud-set-override-redirect.c -lX11',
    { stdio: 'inherit' }
  );
} catch {
  console.warn('Warning: could not compile hud-set-override-redirect (libx11-dev missing?)');
}

console.log('Build complete.');
