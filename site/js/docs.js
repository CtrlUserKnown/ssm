(function () {
  var navHeight = 80;
  var sections = document.querySelectorAll('.docs-content section[id]');
  var links = document.querySelectorAll('.docs-sidebar a');

  if (!sections.length || !links.length) return;

  function onScroll() {
    var scrollY = window.scrollY + navHeight + 20;
    var current = '';

    sections.forEach(function (section) {
      if (section.offsetTop <= scrollY) {
        current = section.getAttribute('id');
      }
    });

    links.forEach(function (link) {
      link.classList.remove('active');
      if (link.getAttribute('href') === '#' + current) {
        link.classList.add('active');
      }
    });
  }

  window.addEventListener('scroll', onScroll, { passive: true });
  onScroll();
})();
