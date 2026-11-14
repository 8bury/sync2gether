import { Stage } from '../../kit'
import { Prep, type File } from '../s2g-prep/Prep'

// sync2gether, the film step when the copies differ. A still: the seam says
// "not the same", the one fact that differs (the size) is the only thing in
// warning color on both cards, and the note says it in a sentence with what
// to do. The main button keeps its place and says what unblocks it.
// Nothing is said three times: no banner, no per-row error, no second line.

const you: File = { name: 'Filme de demonstração.mkv', size: '4,2 GB', progress: 1, state: 'ok' }
const ana: File = { name: 'Filme.de.demonstracao.720p.mp4', size: '2,1 GB', progress: 1, state: 'bad' }

export default function Mismatch() {
  return (
    <Stage width={880} height={560} theme="dark" initial={{}} className="s2g">
      {() => <Prep you={you} ana={ana} compare="different" main="fix" />}
    </Stage>
  )
}
