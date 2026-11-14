import type { CSSProperties, ReactNode } from 'react'
import { Check as Tick, Settings } from 'lucide-react'
import { Window } from '../../kit'
import './look.css'

// Shared pieces of the sync2gether prototype. The flow has three named
// steps, shown in the header instead of a different big title on every
// screen: the room (invite someone), the film (both pick and verify a
// copy), watching.

export const STEPS = ['Sala', 'Filme', 'Assistir'] as const

export function AppWindow({ step, room, className, style, children }: { step?: number; room?: string; className?: string; style?: CSSProperties; children: ReactNode }) {
  return (
    <Window title="sync2gether" className={`s2g-win${className ? ` ${className}` : ''}`} style={style}>
      <div className="s2g-head" data-bare={step === undefined || undefined}>
        <Steps step={step} />
        <span className="s2g-head__end">
          <span className="s2g-room" data-hidden={!room || undefined}>
            <Presence />
            <span>Sala <b>{room ?? '4821-gato-lua'}</b></span>
          </span>
          <span className="s2g-gear" data-target="gear"><Settings size={16} strokeWidth={1.8} /></span>
        </span>
      </div>
      {children}
    </Window>
  )
}

/** Both people are in the room: one green dot, the way a call shows it's live. */
function Presence() {
  return <i className="s2g-live" aria-hidden />
}

export function Steps({ step }: { step?: number }) {
  return (
    <span className="s2g-steps" data-hidden={step === undefined || undefined}>
      {STEPS.map((name, i) => (
        <span key={name} style={{ display: 'contents' }}>
          {i > 0 && <span className="s2g-step__rule" />}
          <span className="s2g-step" data-state={step === undefined ? 'next' : i < step ? 'done' : i === step ? 'now' : 'next'}>
            <span className="s2g-step__num">
              <span className="s2g-step__n">{i + 1}</span>
              <Tick className="s2g-step__tick" size={12} strokeWidth={2.6} />
            </span>
            {name}
          </span>
        </span>
      ))}
    </span>
  )
}

/** The app's own mark (src/ui/visuals.rs): two screens, the front one playing. */
export function AppMark({ size = 56 }: { size?: number }) {
  return (
    <span className="s2g-mark" style={{ width: size, height: size }}>
      <svg viewBox="0 0 120 88" width={size * 0.6} height={size * 0.44} aria-hidden>
        <rect x="7" y="7" width="76" height="54" rx="10" fill="rgb(84 116 248)" stroke="#fff" strokeWidth="7" />
        <rect x="37" y="27" width="76" height="54" rx="10" fill="rgb(84 116 248)" stroke="#fff" strokeWidth="7" />
        <path d="M68 42 L88 54 L68 66 Z" fill="#fff" />
      </svg>
    </span>
  )
}
