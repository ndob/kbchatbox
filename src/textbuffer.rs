use std::cmp;
use std::collections::vec_deque::VecDeque;

pub struct TextBuffer {
    xsize: usize,
    ysize: usize,
    raw_lines: VecDeque<String>,
}

impl TextBuffer {
    pub fn new(max_x_chars: usize, max_y_chars: usize) -> Self {
        TextBuffer {
            xsize: max_x_chars,
            ysize: max_y_chars,
            raw_lines: VecDeque::new(),
        }
    }

    pub fn append(&mut self, new_line: &str) {
        self.raw_lines.push_back(new_line.to_string());

        // We only need maximum of ysize rows to fill the buffer vertically.
        while self.raw_lines.len() > self.ysize {
            self.raw_lines.pop_front();
        }
    }

    pub fn clear(&mut self) {
        self.raw_lines.clear();
    }

    pub fn get_newest_formatted(&self) -> String {
        return self.get_newest().join("\n");
    }

    fn get_newest(&self) -> Vec<String> {
        let mut formatted: Vec<String> = Vec::new();
        // Iterate from newest to oldest.
        for line in self.raw_lines.iter().rev() {
            // Is the buffer full?
            if formatted.len() >= self.ysize {
                break;
            }

            // Does the new raw line fit as is? If not split into sub lines.
            if line.len() >= self.xsize {
                let new_lines = self.split_into_sublines(line, self.xsize);

                // Check that the lines fit into remaining free lines.
                let truncated_new_lines: Vec<String> = new_lines
                    .iter()
                    .rev()
                    .take(cmp::min(self.ysize - formatted.len(), new_lines.len()))
                    .cloned()
                    .collect();
                formatted.extend(truncated_new_lines);
                continue;
            }
            formatted.push(line.to_string());
        }

        formatted.reverse();
        return formatted;
    }

    fn split_into_sublines(&self, line: &String, max_len: usize) -> Vec<String> {
        let mut ret: Vec<String> = Vec::new();
        let mut it = line.chars();
        loop {
            let new_line = it.by_ref().take(max_len).collect::<String>();
            if new_line.is_empty() {
                break;
            }
            ret.push(new_line);
        }
        return ret;
    }

    #[cfg(test)]
    fn get_raw_buffer_capacity(&self) -> usize {
        return self.raw_lines.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_textbuffer_long() {
        let w = 100;
        let h = 10;
        let mut text_buf = TextBuffer::new(w, h);
        text_buf.append("Conversation start.");
        text_buf.append(" Lorem ipsum dolor sit amet, consectetur adipiscing elit. Curabitur elementum quam quis felis facilisis, a gravida ex posuere. Nunc rutrum erat sed augue volutpat, vel rutrum metus cursus. Vestibulum rutrum lobortis ante, eu placerat lectus rutrum vitae. Praesent ut orci ut lectus pulvinar rutrum. Ut ullamcorper accumsan nunc, ut venenatis mi lacinia non. Aenean iaculis purus mauris, eu ornare ante cursus et. Phasellus eu mauris suscipit, vulputate justo non, consequat erat. Cras non quam id massa mollis efficitur. Suspendisse potenti. In condimentum dignissim nisi, sit amet lobortis dolor tempus ut. Curabitur id aliquet risus, sit amet sodales quam. Orci varius natoque penatibus et magnis dis parturient montes, nascetur ridiculus mus. Sed venenatis ac felis et vulputate.");
        text_buf.append("Sed a lacinia mi. Mauris id felis non felis aliquet finibus. Etiam efficitur dui non sagittis elementum. Curabitur viverra non quam vel tincidunt. Nullam eleifend, sem sit amet tincidunt rhoncus, enim nulla condimentum dui, eu pulvinar diam risus at urna. Vivamus sollicitudin pharetra elit, ut interdum est accumsan at. Quisque eget nisl pellentesque, condimentum ipsum nec, condimentum dolor. In hac habitasse platea dictumst.");
        let lines = text_buf.get_newest();
        assert_eq!(lines.len(), h);
        for line in lines {
            assert_eq!(line.len() <= 100, true);
        }

        let formatted = text_buf.get_newest_formatted();
        assert_eq!(formatted.len() <= w * h, true);
    }

    #[test]
    fn test_textbuffer_order() {
        let mut text_buf = TextBuffer::new(100, 5);
        for i in 0..20 {
            text_buf.append(&i.to_string());
        }

        let newest = text_buf.get_newest();
        let mut it = newest.iter();
        assert_eq!(it.next().unwrap(), "15");
        assert_eq!(it.next().unwrap(), "16");
        assert_eq!(it.next().unwrap(), "17");
        assert_eq!(it.next().unwrap(), "18");
        assert_eq!(it.next().unwrap(), "19");
        assert_eq!(None, it.next());
    }

    #[test]
    fn test_textbuffer_spill_over() {
        // Checks that TextBuffer does not leak memory by not
        // purging the old values that are not needed anymore.
        let h = 10;
        let mut text_buf = TextBuffer::new(100, h);
        assert_eq!(text_buf.get_raw_buffer_capacity(), 0);
        for i in 0..3 {
            text_buf.append(&i.to_string());
        }

        assert_eq!(text_buf.get_raw_buffer_capacity(), 3);

        for i in 0..50 {
            text_buf.append(&i.to_string());
        }

        assert_eq!(text_buf.get_raw_buffer_capacity(), h);
    }

}
