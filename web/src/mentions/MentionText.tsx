import { useMentions } from './context';
import './mentions.css';
export function MentionText({ id }: { id: string }) {
  const value = useMentions()?.get(id); const available = value?.available && !!value.display_name;
  return <span className={`user-mention${available ? '' : ' unavailable'}`} title={available ? `Mention of ${value.display_name}` : 'Mention unavailable'}>@{available ? value.display_name : 'someone'}</span>;
}
