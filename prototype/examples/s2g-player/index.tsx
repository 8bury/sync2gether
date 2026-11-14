import { Captions, Maximize, Pause, Play, RotateCcw, RotateCw, Volume2 } from 'lucide-react'
import { Avatar, Spinner, Stage, States, Swap, Toast, Window, type Act } from '../../kit'
import '../_s2g/look.css'
import './player.css'

// sync2gether, watching. Beats: the film plays; Ana pauses and a toast says
// who did it and where; the connection drops and the room pill turns to
// Reconectando on itself (no second banner); Ana comes back, the pill
// returns to Em sincronia and one toast says what to do; you press play.
// Plays once; the poster is the film playing with both in sync.
//
// The room's state lives in one pill at the top right, always in the same
// place, instead of an overlay card that covers the film.
//
// Real strings: the title, the pill's states, the two toasts, the time.

type Link = 'sync' | 'lost'
type S = { playing: boolean; t: number; link: Link; toast: 'none' | 'paused' | 'back' }

const initial: S = { playing: true, t: 754, link: 'sync', toast: 'none' }
const poster: S = { playing: true, t: 760, link: 'sync', toast: 'none' }
const LENGTH = 5400

const clock = (t: number) => `${Math.floor(t / 60)}:${String(t % 60).padStart(2, '0')}`

async function script(a: Act<S>) {
  for (let t = 755; t <= 756; t++) {
    await a.wait(900)
    a.put({ t })
  }
  a.mark('paused')
  a.put({ playing: false, toast: 'paused' })
  await a.wait(2200)
  a.put({ toast: 'none' })
  await a.wait(500)
  a.mark('lost')
  a.put({ link: 'lost' })
  await a.wait(2400)
  a.mark('back')
  a.put({ link: 'sync', toast: 'back' })
  await a.wait(1600)
  await a.click('play')
  a.put({ playing: true, toast: 'none' })
  for (let t = 757; t <= 759; t++) {
    await a.wait(1000)
    a.put({ t })
  }
}

export default function Player() {
  return (
    <Stage<S> width={880} height={560} theme="dark" initial={initial} poster={poster} script={script} play="once" cursor={{ at: { x: 640, y: 380 } }} className="s2g w2g">
      {(s) => (
        <Window title="Filme de demonstração — sync2gether" className="s2g-win w2g-win">
          <div className="w2g-video">
            <div className="w2g-top">
              <span className="w2g-title">Filme de demonstração</span>
              <span className="w2g-pill" data-link={s.link}>
                {/* One pill whose content crossfades while its width eases */}
                <Swap id={s.link}>
                  {s.link === 'sync'
                    ? <span className="w2g-pill__in"><Avatar name="Ana" size={20} />Ana · em sincronia</span>
                    : <span className="w2g-pill__in"><span className="w2g-spin"><Spinner size={14} /></span>Reconectando com Ana…</span>}
                </Swap>
              </span>
            </div>
            <div className="w2g-toasts">
              <Toast show={s.toast === 'paused'} icon={<Avatar name="Ana" size={20} />}>Ana pausou em 12:36</Toast>
              <Toast show={s.toast === 'back'} icon={<Avatar name="Ana" size={20} />}>Ana voltou. Deem play quando quiserem.</Toast>
            </div>
            <div className="w2g-controls">
              <div className="w2g-track"><span style={{ width: `${(s.t / LENGTH) * 100}%` }} /></div>
              <div className="w2g-row">
                <span className="w2g-btn w2g-btn--main" data-target="play" data-cursor="pointer">
                  <States value={s.playing ? 'pause' : 'play'} states={{ pause: <Pause size={18} fill="currentColor" />, play: <Play size={18} fill="currentColor" /> }} />
                </span>
                <span className="w2g-btn"><RotateCcw size={18} /></span>
                <span className="w2g-btn"><RotateCw size={18} /></span>
                <span className="w2g-time">{clock(s.t)} <i>/ 1:30:00</i></span>
                <span className="w2g-gap" />
                <span className="w2g-btn"><Volume2 size={18} /></span>
                <span className="w2g-btn"><Captions size={18} /></span>
                <span className="w2g-btn"><Maximize size={18} /></span>
              </div>
            </div>
          </div>
        </Window>
      )}
    </Stage>
  )
}
