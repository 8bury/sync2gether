import { Stage, type Act } from '../../kit'
import { AppWindow } from '../_s2g/parts'
import { Home, type Join } from '../_s2g/home'
import { PrepBody, type File } from '../s2g-prep/Prep'
import '../s2g-prep/prep.css'
import './guest.css'

// sync2gether, the guest's way in. Beats: the code is typed straight into
// the start screen with a word wrong; Entrar answers in the field's own lane
// and ring; the word is fixed and the ring calms; Entrar carries its own
// progress (Entrando, then Entrou); the start screen hands over to the film
// step, where Ana's copy is already waiting. Plays once; the poster is the
// guest arriving in the film step.
//
// Real strings: the title, the code as typed, the error, the button states,
// then the film step's own words.

type S = { screen: 'home' | 'film'; code: string; focused: boolean; join: Join }

const initial: S = { screen: 'home', code: '', focused: false, join: 'idle' }
const poster: S = { screen: 'film', code: '4821-gato-lua', focused: false, join: 'joined' }

const ana: File = { name: 'Filme.de.demonstracao.1080p.mkv', size: '4,2 GB', progress: 1, state: 'ok' }
const none: File = { name: '', size: '', progress: 0, state: 'none' }

async function script(a: Act<S>) {
  await a.wait(700)
  a.mark('type')
  await a.click('code')
  a.put({ focused: true })
  await a.type('code', '4821-gato-rua', { interval: 70 })
  await a.wait(300)
  await a.click('enter')
  a.put({ join: 'connecting' })
  await a.wait(900)
  a.mark('error')
  a.put({ join: 'error' })
  await a.wait(1700)
  a.mark('fix')
  // Only the wrong word is retyped
  for (const t of ['4821-gato-ru', '4821-gato-r', '4821-gato-', '4821-gato-l', '4821-gato-lu', '4821-gato-lua']) {
    a.put({ code: t })
    await a.wait(90)
  }
  a.put({ join: 'idle' })
  await a.wait(500)
  await a.click('enter')
  a.put({ join: 'connecting', focused: false })
  await a.wait(1300)
  a.mark('joined')
  a.put({ join: 'joined' })
  await a.wait(700)
  a.set({ screen: 'film' })
  await a.wait(2600)
}

export default function Guest() {
  return (
    <Stage<S> width={880} height={560} theme="dark" initial={initial} poster={poster} script={script} play="once" cursor={{ at: { x: 700, y: 500 } }} className="s2g">
      {(s) => (
        <AppWindow step={s.screen === 'film' ? 1 : undefined} room={s.screen === 'film' ? '4821-gato-lua' : undefined} className="g2g-win">
          <div className="s2g-screens g2g-body">
            <Home on={s.screen === 'home'} code={s.code} focused={s.focused} join={s.join} />
            <section className="s2g-screen" data-on={s.screen === 'film' || undefined}>
              <PrepBody you={none} ana={ana} compare="waiting" main="choose" />
            </section>
          </div>
        </AppWindow>
      )}
    </Stage>
  )
}
