import { Button, cx } from '@stump/components'
import { SortAsc } from 'lucide-react'

type Direction = 'asc' | 'desc'
type Props = {
	value?: Direction
	onChange: (value: Direction) => void
	disabled?: boolean
}
export default function OrderByDirection({ value, onChange, disabled }: Props) {
	return (
		<Button
			variant="ghost"
			className="justify-start"
			disabled={disabled}
			onClick={() => onChange(value === 'desc' ? 'asc' : 'desc')}
		>
			<SortAsc
				className={cx('mr-1.5 h-4 w-4 text-foreground-muted transition-all', {
					'rotate-180': value === 'desc',
				})}
			/>
			{value === 'desc' ? 'Descending' : 'Ascending'}
		</Button>
	)
}
