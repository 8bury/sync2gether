import { Stage, type Act } from '../../kit'
import { Prep, type Compare, type File, type Main } from './Prep'

// sync2gether, the film step for the host. Beats: Ana's copy is already
// checked; you choose your file and it is checked in place, its progress on
// its own line; the seam between the copies turns to "=" and the title to
// Tudo pronto; the main button, which said what was missing all along,
// lights up as Começar a assistir; a click starts the countdown and the
// stepper moves to Assistir. Plays once; the poster is the ready screen.
//
// Real strings: the title, both file names and sizes, the status lines, the
// main button.

type S = { you: File; compare: Compare; main: Main; count: number; step: number }

const ana: File = { name: 'Filme.de.demonstracao.1080p.mkv', size: '4,2 GB', progress: 1, state: 'ok' }
const mine = (state: File['state'], progress: number): File => ({ name: 'Filme de demonstração.mkv', size: '4,2 GB', progress, state })

const initial: S = { you: mine('none', 0), compare: 'waiting', main: 'choose', count: 3, step: 1 }
const poster: S = { you: mine('ok', 1), compare: 'same', main: 'start', count: 3, step: 1 }

async function script(a: Act<S>) {
  await a.wait(900)
  a.mark('choose')
  await a.click('choose')
  a.set({ you: mine('checking', 0.08), compare: 'checking', main: 'checking' })
  await a.move({ at: 'start', x: 0.85, y: 2.2 })
  for (const p of [0.24, 0.41, 0.6, 0.78, 0.93]) {
    await a.wait(380)
    a.put({ you: mine('checking', p) })
  }
  await a.wait(380)
  a.mark('same')
  a.put({ you: mine('ok', 1), compare: 'same' })
  await a.wait(500)
  a.put({ main: 'start' })
  await a.wait(1300)
  a.mark('start')
  await a.click('start')
  a.put({ main: 'starting', count: 3 })
  for (const n of [2, 1]) {
    await a.wait(800)
    a.put({ count: n })
  }
  await a.wait(500)
  a.put({ step: 2 })
  await a.wait(2200)
}

export default function PrepScene() {
  return (
    <Stage<S> width={880} height={560} theme="dark" initial={initial} poster={poster} script={script} play="once" cursor={{ at: { x: 720, y: 520 } }} className="s2g">
      {(s) => <Prep you={s.you} ana={ana} compare={s.compare} main={s.main} count={s.count} step={s.step} />}
    </Stage>
  )
}
