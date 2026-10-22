import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from 'react';
import { Modal } from '../components';

interface Question { title: string; description: string; action: string }
type Confirm = (question: Question) => Promise<boolean>;
const Context = createContext<Confirm | null>(null);
export function useNativeConfirmation(optional: true): Confirm | null;
export function useNativeConfirmation(optional?: false): Confirm;
export function useNativeConfirmation(optional = false): Confirm | null { const value = useContext(Context); if (!value && !optional) throw new Error('Native confirmation is unavailable.'); return value; }

export function ConfirmActionProvider({ children }: { children: ReactNode }) {
  const [question, setQuestion] = useState<Question | null>(null);
  const answer = useRef<((confirmed: boolean) => void) | null>(null);
  useEffect(() => () => { answer.current?.(false); answer.current = null; }, []);
  function finish(confirmed: boolean) { const resolve = answer.current; answer.current = null; setQuestion(null); resolve?.(confirmed); }
  function confirm(next: Question) { if (answer.current) return Promise.resolve(false); setQuestion(next); return new Promise<boolean>(resolve => { answer.current = resolve; }); }
  return <Context.Provider value={confirm}>{children}{question && <Modal title={question.title} close={() => finish(false)}><div className="settings-modal-content"><p>{question.description}</p><div className="modal-footer"><button type="button" className="button" data-autofocus onClick={() => finish(false)}>Cancel</button><button type="button" className="button primary" onClick={() => finish(true)}>{question.action}</button></div></div></Modal>}</Context.Provider>;
}
