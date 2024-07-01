"use strict"

function main() {
	document.removeEventListener("DOMContentLoaded", main, false)

	// NOTE: this is a hacky solution but should do the job for making a
	// listenable audio link
	function playAudio(evt) {
		evt.preventDefault()
		var audio = document.createElement("audio")
		audio.addEventListener("ended", function() { audio.delete() }, false)
		audio.addEventListener("loadeddata", function() { audio.play() }, false)
		audio.src = evt.target.href
		audio.load()
	}

	var audios = document.querySelectorAll("a[href^=\"data:audio/\"]")
	Array.prototype.forEach.call(audios, function setupAudio(elem) {
		elem.setAttribute("role", "button")
		elem.addEventListener("click", playAudio, false)
	})
}

document.addEventListener("DOMContentLoaded", main, false)
