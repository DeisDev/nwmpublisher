<script>
	import { playSound, stopSound } from "../sounds";

	export let id;
	export let value = false;
	export let beforeChange = null;
	export let afterChange = null;

	let checked = value;

	let checkbox;
	async function toggle(e) {
		e.preventDefault();
		e.stopPropagation();

		const newValue = !checked;

		if (beforeChange) {
			const override = await beforeChange(newValue);
			if (override != newValue) return;
		}

		checkbox.checked = newValue;
		checked = newValue;

		stopSound('btn-on');
		stopSound('btn-off');
		playSound(checked ? 'btn-on' : 'btn-off');

		if (afterChange) afterChange.call(checkbox);
	}
</script>

<span class="switch-container">
	<label id={id + '-label'} for={id + '-switch'}><slot></slot></label>
	<button type="button" id={id + '-switch'} class="switch" role="switch" aria-checked={checked} aria-labelledby={id + '-label'} on:click={toggle} class:checked={checked}>
		<span class="circle"></span>
	</button>
	<input type="checkbox" id={id} name={id} checked={value ? true : null} bind:this={checkbox}/>
</span>

<style>
	.switch-container {
		display: inline-flex;
		cursor: pointer;
	}
	.switch-container > label {
		margin-right: .4rem;
		cursor: pointer;
	}

	.switch-container > input {
		visibility: hidden;
		position: absolute;
		width: 0;
		height: 0;
		pointer-events: none;
		-webkit-pointer-events: none;
	}
	.switch {
		position: relative;
		width: 2.5rem;
		height: 1.1rem;
		border-radius: 1rem;
		transition: background-color .25s;
		display: inline-flex;
		box-shadow: inset 0 0 3px 0px rgb(0 0 0 / 50%);
		border: 0;
		padding: 0;
		cursor: pointer;
	}
	.switch:focus-visible {
		outline: 2px solid #127cff;
		outline-offset: 2px;
	}
	.switch > .circle {
		position: absolute;
		top: 0;
		left: 0;
		border-radius: 100%;
		width: .9rem;
		height: .9rem;
		margin: .1rem;
		background-color: #fff;
		transition: left .25s;
	}
	.switch.checked {
		background-color: #009aff;
	}
	.switch:not(.checked) {
		background-color: #949494;
	}
	.switch.checked > .circle {
		left: calc(100% - .9rem - .2rem);
	}
	.switch:not(.checked) > .circle {
		left: 0;
	}
</style>
