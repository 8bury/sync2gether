import { Copy, Check as Tick, FolderOpen } from 'lucide-react'
import { Avatar, Button, Roll, Stage, States, type Act } from '../../kit'
import { AppWindow } from '../_s2g/parts'
import { Home } from '../_s2g/home'
import './host.css'

// sync2gether, the host's first minute. Beats: the start screen offers both
// ways in at once (create, or paste a code right there); Criar sala opens the
// room, whose one job is the code, set large; Copiar turns to Copiado on the
// same button; the partner's ring goes from waiting to confirmed and the line
// beside it rolls to "Ana entrou"; the step moves on and the next action
// lights up. Plays once; the poster is the joined room.
//
// Redesign notes against the current egui screens:
// - Joining has no screen of its own: the code field sits on the start screen.
// - The room screen is about the code, so the code is the one big thing; the
//   film choice waits below as a quiet secondary action you can take early.
// - Where you are is said once, by the stepper, not by a title per state.
//
// Real strings: the title, the two actions, the code, the presence line, the
// next action.

type S = { screen: 'home' | 'room'; copied: boolean; joined: boolean }

const initial: S = { screen: 'home', copied: false, joined: false }
const poster: S = { screen: 'room', copied: true, joined: true }

async function script(a: Act<S>) {
  await a.wait(900)
  a.mark('create')
  await a.click('create')
  a.set({ screen: 'room' })
  await a.wait(1100)
  a.mark('copy')
  await a.click('copy')
  a.put({ copied: true })
  await a.move({ at: 'copy', x: 0.6, y: 2.4 })
  await a.wait(1700)
  a.mark('joined')
  a.put({ joined: true })
  await a.wait(900)
  await a.move({ at: 'choose', x: 0.7, y: 0.7 })
  await a.wait(2600)
}

export default function Host() {
  return (
    <Stage<S> width={880} height={560} theme="dark" initial={initial} poster={poster} script={script} play="once" cursor={{ at: { x: 700, y: 500 } }} className="s2g h2g">
      {(s) => (
        <AppWindow step={s.screen === 'home' ? undefined : s.joined ? 1 : 0} room={s.joined ? '4821-gato-lua' : undefined} className="h2g-win">
          <div className="s2g-screens h2g-body">
            <Home on={s.screen === 'home'} />

            <section className="s2g-screen h2g-room" data-on={s.screen === 'room' || undefined}>
              <div className="h2g-label">Envie este código para a outra pessoa</div>
              <div className="h2g-code">
                <span className="h2g-code__text">4821-gato-lua</span>
                <Button variant={s.copied ? 'secondary' : 'primary'} className="s2g-cta h2g-copy" data-target="copy" data-cursor="pointer" icon={<States value={s.copied ? 'on' : 'off'} states={{ off: <Copy />, on: <Tick /> }} />}>
                  <Roll variant="fade">{s.copied ? 'Copiado' : 'Copiar'}</Roll>
                </Button>
              </div>
              <div className="h2g-people">
                <Avatar name="Você" size={28} />
                <span className="h2g-link" data-on={s.joined || undefined} />
                <Avatar name={s.joined ? 'Ana' : ''} size={28} tone={s.joined ? 'person' : 'ink'} ring={s.joined ? 'confirmed' : 'waiting'} />
                <span className="h2g-presence" data-on={s.joined || undefined}>
                  <Roll variant="fade">{s.joined ? 'Ana entrou na sala' : 'Esperando alguém entrar…'}</Roll>
                </span>
              </div>
              <div className="h2g-next">
                <Button variant={s.joined ? 'primary' : 'secondary'} className="s2g-cta h2g-wide" icon={<FolderOpen />} data-target="choose" data-cursor="pointer">Escolher meu filme</Button>
                <span className="s2g-link">Encerrar sala</span>
              </div>
            </section>
          </div>
        </AppWindow>
      )}
    </Stage>
  )
}
